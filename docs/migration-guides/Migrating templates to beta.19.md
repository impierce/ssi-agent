# Migrating Guide to `beta.19`

This guide is for developers who want to migrate to the `beta.19` release of the UniCore API. Some breaking changes have been made to the templates API which are outlined below.

## Improvements

- **Stronger template enforcement**: Fields like `dataModel`, `holderType`, and `credentialExpiration` are more strictly validated.
- **No more manual credential configurations**: There is no need to manage the "credential configurations" manually to go in sync with the template you created. Credential configurations are automatically kept in sync with the template.
- **Clearer endpoint names**: template routes now follow a "command-style" naming convention and are flat under `/v0` paths without paths such as `/v0/templates/`.

---

## 1. Rename template endpoints

All template routes have moved from `/v0/templates/*` to `/v0/*` paths and changed their names slightly.

| Previous                                | New                               |
| --------------------------------------- | --------------------------------- |
| `GET /v0/templates/get-all-templates`   | `GET /v0/list-all-templates`      |
| `GET /v0/templates/{template_id}`       | `GET /v0/get-template-by-id/{id}` |
| `POST /v0/templates/create-template`    | `POST /v0/create-new-template`    |
| `POST /v0/templates/update-template`    | `POST /v0/update-template`        |
| `POST /v0/templates/duplicate-template` | `POST /v0/duplicate-template`     |
| `POST /v0/templates/delete-template`    | `POST /v0/delete-template`        |

---

## 2. Remove credential configuration calls

The management of **credential configurations** is no longer required and you can remove any calls to `POST /v0/credential-configurations`. When you create or update a template, UniCore automatically derives and publishes the matching credential configuration.

---

## 3. Update template creation requests

The `create-new-template` request body has changed:

- `creator` field removed.
- `credentialExpiration` added (optional). Accepts `never`, `duration` (ISO 8601), or `date-time` (ISO 8601). The default value is **90 days** if not stated explicitly.
- `dataModel` defaults to `w3c_vc_data_model_v2-0` if omitted and can not be changed after initial creation.
- `schema` is more restrictive when the `dataModel` is following a standard such as **Open Badges 3.0**.
- `schemaPropertiesAttributes` is optional, but only required if some fields in the schema should be selectively disclosable.
- `holderType` defaults to `individual` if omitted and can not be changed after initial creation.

**Before:**

```http
POST /v0/templates/create-template
Content-Type: application/json

{
  "title": "Employee ID Credential",
  "dataModel": "w3c_vc_data_model_v1-1",
  "holderType": "individual",
  "creator": "acme-corp"
}
```

**After:**

```http
POST /v0/create-new-template
Content-Type: application/json

{
  "title": "Employee ID Credential",
  "dataModel": "w3c_vc_data_model_v1-1",
  "holderType": "individual",
  "credentialExpiration": { "type": "duration", "value": "P1Y" },
  "schema": {
    "type": "object",
    "properties": {
      "name": { "type": "string" },
      "employeeId": { "type": "string" }
    }
  }
}
```

---

## 4. Update credential creation requests

`POST /v0/credentials` no longer accepts `credentialConfigurationId`. Replace it with `templateId`, which is the `id` returned when you created the template. Credential expiration is now determined by the template's `credentialExpiration` field, but can be overridden by specifying `expiresAt` during issuance (as long as it does not exceed the template limit).

**Before:**

```http
POST /v0/credentials
Content-Type: application/json

{
  "offerId": "...",
  "credentialConfigurationId": "employee-id-credential",
  "expiresAt": "2027-01-01T00:00:00Z",
  "credential": { ... }
}
```

**After:**

```http
POST /v0/credentials
Content-Type: application/json

{
  "offerId": "...",
  "templateId": "550e8400-e29b-41d4-a716-446655440000",
  "credential": { ... }
}
```
