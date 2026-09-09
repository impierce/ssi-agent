# UniCore Bruno collection

This directory contains the tooling used to generate a [Bruno](https://www.usebruno.com/)
collection from `../openapi.yaml`.

## Open the collection

In Bruno, choose **Open Collection** and select the `agent_api_http/bruno/gen` directory.

Although the directory is named `gen`, Bruno displays the collection as **UniCore HTTP API**.
The display name comes from the generated `gen/bruno.json` and `gen/collection.bru` files, not
from the directory name.

Requests use `BASE_URL` and `API_KEY`. The generated **Local development** environment includes
`BASE_URL` with the default `http://localhost:3033` value and an empty `API_KEY` secret, but marks
both inactive. Bruno therefore uses values from the Global environment by default. Activate either
variable in Bruno when a local override is needed. The remaining uppercase environment variables are
populated by post-response scripts where possible and can also be set manually.

## Regenerate the collection

Node.js with `npx` must be available. From the repository root, run:

```shell
cd agent_api_http
cargo test generate_openapi_spec
./bruno/generate.sh
```

The script imports the OpenAPI specification with `@usebruno/cli`, applies UniCore-specific
patches, and synchronizes the result into `bruno/gen`.

Treat the generated requests, folders, `bruno.json`, `collection.bru`, and the **Local
development** environment in `gen` as generated files. Direct edits to them are overwritten the
next time `generate.sh` runs. Additional files in `gen/environments` are preserved so locally
maintained staging or production environments can coexist with the generated environment.

## What to modify

- To change endpoint names, grouping, documentation, schemas, examples, or responses, update the
  endpoint's `utoipa` annotations in `src` and ensure it is registered in the appropriate
  `openapi.rs` module. Then regenerate the OpenAPI document and Bruno collection.
- To change the generated collection's display name, update `--collection-name` in `generate.sh`.
- To change Bruno's generated local `BASE_URL`, update the server declaration in `src/v0/openapi.rs`
  and regenerate.
- To add API authentication, chained-request variables, path-variable defaults, or response
  extraction that OpenAPI cannot express, update `patch-collection.mjs`. Rules are matched by HTTP
  method and normalized path.
- To add another generated environment variable, update `ENV_VARS` in `patch-collection.mjs`.

Do not make durable customizations directly in `gen`; encode them in the OpenAPI annotations,
`generate.sh`, or `patch-collection.mjs` instead.
