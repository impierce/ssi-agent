#!/usr/bin/env python3
"""Wait for a fuzz instance to become ready, seed it with resources, and write their identifiers as
Schemathesis dictionaries.

Used by `scripts/fuzz-openapi.sh` before the fuzz run, from inside the compose network:

    python3 fuzz-seed.py http://ssi-agent:3033 /reports/seed [ready timeout in seconds]

Without existing resources, every operation that takes an identifier answers `404` and its handler
logic is never reached. `agent_application/docker/schemathesis.toml` binds the identifier
parameters to the dictionaries written here, so part of the generated requests use identifiers that
exist while the rest stays random.

Holder credentials, holder offers, presentations, connections and reissuances are not seeded: they
need a remote issuer, wallet or agent, which a hermetic run does not have.
"""

import json
import sys
import time
import urllib.error
import urllib.request
import uuid

# Each seeded kind gets a few instances, so operations that delete or change state (`delete-template`,
# `take-public-offer-offline`, ...) do not use up the identifiers for the rest of the run.
COPIES = 3

base_url, out_dir = sys.argv[1].rstrip("/"), sys.argv[2]
ready_timeout = int(sys.argv[3]) if len(sys.argv) > 3 else 120


def wait_until_ready():
    deadline = time.monotonic() + ready_timeout
    while True:
        try:
            with urllib.request.urlopen(base_url + "/readyz", timeout=5) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, OSError):
            pass
        if time.monotonic() >= deadline:
            sys.exit(f"The agent did not become ready within {ready_timeout}s.")
        time.sleep(2)


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base_url + path, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            raw = response.read()
            is_json = "json" in response.headers.get("Content-Type", "")
            return response.status, response.headers, json.loads(raw) if raw and is_json else None
    except urllib.error.HTTPError as error:
        sys.exit(f"Seeding failed: {method} {path} returned {error.code}: {error.read().decode(errors='replace')}")


def template(title, schema):
    _, _, body = request(
        "POST",
        "/v0/create-new-template",
        {
            "title": title,
            "status": "published",
            "dataModel": "w3c_vc_data_model_v2-0",
            "holderType": "individual",
            "visibility": "private",
            "type": ["VerifiableCredential"],
            "holderAuthorization": {"pre_authorized": True},
            "schema": {"type": "object", "properties": schema},
        },
    )
    return body["id"]


def offer(template_id):
    offer_id = str(uuid.uuid4())
    request("POST", "/v0/offers", {"offerId": offer_id, "templateIds": [template_id]})
    return offer_id


wait_until_ready()
print("==> Agent is ready, seeding")

seeded = {name: [] for name in ("templates", "catalogs", "offers", "credentials", "authorization_requests")}

for copy in range(COPIES):
    template_id = template(f"Fuzz template {copy}", {"name": {"type": "string", "title": "Name"}})
    # Public offers only accept templates whose fields all have constant values.
    public_template_id = template(f"Fuzz public template {copy}", {"name": {"type": "string", "const": "Fuzz"}})
    seeded["templates"] += [template_id, public_template_id]

    _, _, catalog = request(
        "POST",
        "/v0/create-new-catalog",
        {"display": {"name": f"Fuzz catalog {copy}", "description": "Seeded"}, "visibility": "Public"},
    )
    seeded["catalogs"].append(catalog["id"])

    offer_id = offer(template_id)
    _, headers, _ = request(
        "POST",
        "/v0/credentials",
        {"offerId": offer_id, "templateId": template_id, "credential": {"credentialSubject": {"name": "Fuzz"}}},
    )
    seeded["offers"].append(offer_id)
    seeded["credentials"].append(headers["Location"].rsplit("/", 1)[-1])

    public_offer_id = offer(public_template_id)
    request("POST", "/v0/create-public-offer", {"offerId": public_offer_id, "templateId": public_template_id})
    seeded["offers"].append(public_offer_id)

    _, headers, _ = request("POST", "/v0/authorization_requests", {})
    seeded["authorization_requests"].append(headers["Location"].rsplit("/", 1)[-1])

request("POST", "/v0/add-key-provider", {"providerType": "in_memory", "name": "Fuzz"})

# Created at startup.
_, _, documents = request("GET", "/v0/documents")
seeded["documents"] = [document["id"] for document in documents]

for name, values in seeded.items():
    with open(f"{out_dir}/{name}.dict", "w", encoding="utf-8") as file:
        file.writelines(f"{json.dumps(value)}\n" for value in values)
    print(f"    {name}: {len(values)}")
