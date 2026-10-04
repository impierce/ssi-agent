#!/usr/bin/env python3
"""Render a Schemathesis JUnit report as a Markdown summary.

Used by the `Fuzz OpenAPI` workflow to fill in the job summary, and usable on its own:

    python3 scripts/fuzz-summary.py reports/fuzz/junit.xml

Schemathesis packs every finding for an operation into a single `<failure>` message, one
`- <check name>` bullet per finding, so the counts come from parsing those bullets.
"""

import re, sys, collections, xml.etree.ElementTree as ET

root = ET.parse(sys.argv[1]).getroot()
suites = root.iter("testsuite") if root.tag == "testsuites" else [root]

rows = []
totals = collections.Counter()
total = 0

for suite in suites:
    for case in suite.iter("testcase"):
        total += 1
        problems = [c for c in case if c.tag in ("failure", "error")]
        if not problems:
            continue
        text = "\n".join((p.get("message") or p.text or "") for p in problems)
        checks = collections.Counter(re.findall(r"^- (.+)$", text, re.M))
        if not checks:
            checks = collections.Counter({problems[0].tag: len(problems)})
        totals.update(checks)
        detail = ", ".join(f"{name} ({n})" for name, n in checks.most_common())
        rows.append((sum(checks.values()), case.get("name", "?"), detail))

print("## Fuzz results\n")

if not rows:
    plural = "operation" if total == 1 else "operations"
    print(f"All {total} {plural} conformed to the specification.")
    sys.exit()

print(f"**{len(rows)} of {total} operations reported findings.**\n")
print("| Finding | Count |")
print("| --- | --- |")
for name, n in totals.most_common():
    print(f"| {name} | {n} |")

print("\n| Operation | Findings |")
print("| --- | --- |")
for _, name, detail in sorted(rows, reverse=True):
    print(f"| `{name}` | {detail} |")
