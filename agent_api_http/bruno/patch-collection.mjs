#!/usr/bin/env node
// Re-injects the bits `bru import openapi` can't produce because OpenAPI has no
// concept of them: API-key auth, chained-request variables, and the post-response
// scripts that extract response data into those variables.
//
// Rules are matched against generated requests by (HTTP method, URL path with
// path-params normalized), not by filename or folder. Filenames/folders are
// derived from the OpenAPI `summary`/`tags` fields and can shift on regeneration;
// paths are the stable identity tied to the actual API contract.
//
// Invoked by generate.sh. Not meant to be run standalone.

import fs from 'node:fs';
import path from 'node:path';

const [, , collectionDir] = process.argv;
if (!collectionDir) {
  console.error('Usage: patch-collection.mjs <collection-dir>');
  process.exit(1);
}

function extractLastIdScript(varName) {
  return [
    "const items = res.getBody();",
    "if (Array.isArray(items) && items.length > 0) {",
    "  const lastId = items[items.length - 1]?.id;",
    "  if (lastId) {",
    `    bru.setVar('${varName}', lastId);`,
    "  }",
    "}",
  ].join('\n');
}

// method + OpenAPI-style path -> what to inject.
//   script:   appended as a script:post-response block
//   pathVars: pre-fills a params:path {} value with a variable reference
const RULES = [
  {
    method: 'POST', path: '/v0/credentials',
    script: [
      "const location = res.getHeader('location');",
      "if (location) {",
      "  bru.setVar('CREDENTIAL_LOCATION', location);",
      "}",
    ].join('\n'),
  },
  {
    method: 'POST', path: '/v0/offers',
    script: [
      "const raw = res.getBody();",
      "const decoded = decodeURIComponent(typeof raw === 'string' ? raw : String(raw));",
      "const [, value] = decoded.split('=', 2);",
      "if (value) {",
      "  bru.setVar('CREDENTIAL_OFFER_URI', value);",
      "}",
    ].join('\n'),
  },
  { method: 'GET', path: '/v0/holder/offers', script: extractLastIdScript('RECEIVED_OFFER_ID') },
  { method: 'POST', path: '/v0/holder/offers/{offer_id}/accept', pathVars: { offer_id: '{{RECEIVED_OFFER_ID}}' } },
  { method: 'POST', path: '/v0/holder/offers/{offer_id}/reject', pathVars: { offer_id: '{{RECEIVED_OFFER_ID}}' } },
  { method: 'GET', path: '/v0/holder/offers/{received_offer_id}', pathVars: { received_offer_id: '{{RECEIVED_OFFER_ID}}' } },

  { method: 'GET', path: '/v0/holder/credentials', script: extractLastIdScript('HOLDER_CREDENTIAL_ID') },
  { method: 'GET', path: '/v0/holder/credentials/{holder_credential_id}', pathVars: { holder_credential_id: '{{HOLDER_CREDENTIAL_ID}}' } },

  { method: 'GET', path: '/v0/holder/presentations', script: extractLastIdScript('PRESENTATION_ID') },
  { method: 'GET', path: '/v0/holder/presentations/{presentation_id}', pathVars: { presentation_id: '{{PRESENTATION_ID}}' } },

  { method: 'GET', path: '/v0/connections', script: extractLastIdScript('CONNECTION_ID') },
  { method: 'GET', path: '/v0/connections/{id}', pathVars: { id: '{{CONNECTION_ID}}' } },

  { method: 'GET', path: '/v0/list-all-templates', script: extractLastIdScript('TEMPLATE_ID') },
  { method: 'GET', path: '/v0/get-template-by-id/{id}', pathVars: { id: '{{TEMPLATE_ID}}' } },

  { method: 'GET', path: '/v0/documents', script: extractLastIdScript('DOCUMENT_ID') },
  { method: 'GET', path: '/v0/documents/{document_id}', pathVars: { document_id: '{{DOCUMENT_ID}}' } },
];

// Declared (with empty placeholder values) in the environment file so they're
// visible/editable even before a script has ever set them.
const ENV_VARS = [
  'CREDENTIAL_LOCATION',
  'CREDENTIAL_OFFER_URI',
  'RECEIVED_OFFER_ID',
  'HOLDER_CREDENTIAL_ID',
  'PRESENTATION_ID',
  'CONNECTION_ID',
  'TEMPLATE_ID',
  'DOCUMENT_ID',
];

const SECRET_ENV_VARS = ['~API_KEY'];

function normalize(urlPath) {
  return urlPath
    .split('/')
    .map((seg) => (seg.startsWith(':') || /^\{.*\}$/.test(seg) ? '*' : seg))
    .join('/');
}

const rulesByKey = new Map();
for (const rule of RULES) {
  rulesByKey.set(`${rule.method} ${normalize(rule.path)}`, rule);
}

function walkBruFiles(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walkBruFiles(full, out);
    } else if (entry.name.endsWith('.bru') && !['folder.bru', 'collection.bru'].includes(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

const files = walkBruFiles(collectionDir);
const matchedKeys = new Set();
const applied = [];

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  const withBaseUrlRenamed = content.replaceAll('{{baseUrl}}', '{{BASE_URL}}');
  const methodMatch = withBaseUrlRenamed.match(/^(get|post|put|patch|delete)\s*\{/m);
  const urlMatch = withBaseUrlRenamed.match(/^\s*url:\s*\{\{BASE_URL\}\}(\S+)/m);
  if (!methodMatch || !urlMatch) continue;

  const method = methodMatch[1].toUpperCase();
  const urlPath = urlMatch[1].split('?')[0];
  const key = `${method} ${normalize(urlPath)}`;
  const rule = rulesByKey.get(key);
  let next = withBaseUrlRenamed;

  if (!rule) {
    if (next !== content) {
      fs.writeFileSync(file, next);
    }
    continue;
  }

  matchedKeys.add(key);

  if (rule.pathVars) {
    for (const [name, value] of Object.entries(rule.pathVars)) {
      const re = new RegExp(`(^\\s*${name}:\\s*)$`, 'm');
      if (re.test(next)) {
        next = next.replace(re, `$1${value}`);
      } else {
        console.warn(`  ! ${key}: path param "${name}" not found in ${path.relative(collectionDir, file)} (spec may have renamed it)`);
      }
    }
  }

  if (rule.script) {
    const indented = rule.script.split('\n').map((l) => '  ' + l).join('\n');
    next = next.trimEnd() + `\n\nscript:post-response {\n${indented}\n}\n`;
  }

  if (next !== content) {
    fs.writeFileSync(file, next);
    applied.push(`${key}  ->  ${path.relative(collectionDir, file)}`);
  }
}

// Collection auth: switch from `mode: none` to the same X-API-KEY header auth
// the hand-maintained Postman collection used.
const collectionBruPath = path.join(collectionDir, 'collection.bru');
let collectionBru = fs.readFileSync(collectionBruPath, 'utf8');
collectionBru = collectionBru.replace(
  /auth \{\s*mode:\s*none\s*\}/,
  'auth {\n  mode: apikey\n}\n\nauth:apikey {\n  key: X-API-KEY\n  value: {{API_KEY}}\n  placement: header\n}'
);
fs.writeFileSync(collectionBruPath, collectionBru);

// Environment vars: replace the OpenAPI-generated `baseUrl` declaration with the
// collection's conventional uppercase name. Prefixing BASE_URL and API_KEY with `~`
// keeps them visible but inactive, allowing Global environment values to apply.
const envPath = path.join(collectionDir, 'environments', 'Local development.bru');
let env = fs.readFileSync(envPath, 'utf8');
env = env.replace(/^\s*(?:baseUrl|BASE_URL|API_KEY):.*\r?\n/gm, '');
env = env.replace(
  /\n\}\s*$/,
  '\n' + ['  ~BASE_URL: http://localhost:3033', ...ENV_VARS.map((v) => `  ${v}: `)].join('\n') + '\n}\n'
);
env += `\nvars:secret [\n${SECRET_ENV_VARS.map((v) => `  ${v}`).join('\n')}\n]\n`;
fs.writeFileSync(envPath, env);

console.log(`Patched ${applied.length} request(s):`);
for (const line of applied) console.log('  ' + line);

const unmatched = [...rulesByKey.keys()].filter((k) => !matchedKeys.has(k));
if (unmatched.length > 0) {
  console.warn(`\n${unmatched.length} rule(s) did not match any generated request (path likely changed or removed in openapi-generated.yaml):`);
  for (const key of unmatched) console.warn('  ' + key);
}
