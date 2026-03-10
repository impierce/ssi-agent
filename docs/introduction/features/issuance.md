## Credential Types and Verifiable Formats

Credentials can be issued in 4 different **Digital Credential Data Formats**:
- [Verifiable Credential Data Model 1.1](https://www.w3.org/TR/vc-data-model-1.1) (VC DM 1.1)
- [Verifiable Credential Data Model 2.0](https://www.w3.org/TR/vc-data-model/) (VC DM 2.0)
- [Open Badges 3.0](https://www.imsglobal.org/spec/ob/v3p0) (OB 3.0)
- [European Digital Credential 3.3](https://op.europa.eu/en/web/eu-vocabularies/dataset/-/resource?uri=http://publications.europa.eu/resource/dataset/snb-model) (ELM 3.3)
> For more resources on these specifications please refer to the `README.md` file at https://github.com/impierce/ssi-agent/tree/beta/agent_library/src/json_schemas/README.md.

Furthermore, there are 3 supported formats in which these credentials can be made verifiable, all of which are **envelopping methods**:
- [W3C Verifiable Credentials 1.1](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-w3c-verifiable-credentials) (`jwt_vc_json`)
- [SD-JWT VC](https://datatracker.ietf.org/doc/draft-ietf-oauth-sd-jwt-vc/) (`dc+sd-jwt`)
- [VC DM 2.0 SD-JWT](https://www.w3.org/TR/vc-jose-cose/#with-sd-jwt) (`vc+sd-jwt`)

Due to certain restrictions in the above mentioned formats, not all data formats can be combined with each envelopping method. The following combinations are possible:
| Envelopping Method | Data Format |
|-------------------|-------------|
| W3C Verifiable Credentials 1.1 | VC DM 1.1 |
| W3C Verifiable Credentials 1.1 | OB 3.0 |
| W3C Verifiable Credentials 1.1 | ELM 3.3 |
| SD-JWT VC | SD-JWT VC |
| VC DM 2.0 SD-JWT | VC DM 2.0 |
| VC DM 2.0 SD-JWT | OB 3.0 |
| VC DM 2.0 SD-JWT | ELM 3.3 |