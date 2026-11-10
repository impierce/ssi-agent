# Credential Formats

This page lists the credential formats UniCore supports for issuance, holding and verification, the data model each one carries, and the specification that defines it. Last checked against the code on the `beta` branch and the specifications listed under [Sources](#sources) in November 2026.

## Data model and securing format

A credential has two separate properties. The **data model** describes the claims and their meaning, for example W3C VC Data Model 1.1 or 2.0, Open Badges 3.0 or the European Learning Model 3.3. The **securing format** describes how the issuer signs those claims, for example a JWT, an SD-JWT or a Data Integrity proof. OpenID4VCI and OpenID4VP name each supported combination with a **format identifier** (such as `jwt_vc_json` or `dc+sd-jwt`), and each format fixes which data models it can carry.

A template's `dataModel` picks the data model. UniCore then picks the format, as shown below.

## Supported today

| Format identifier | `typ` header UniCore sets | Data model carried                                                                                                                             | Selective disclosure                                              | Defined by                                                                                                                                                             | Used by UniCore for                                                                                                                                                      |
| ----------------- | ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `jwt_vc_json`     | `JWT`                     | W3C VCDM 1.1, credential inside the `vc` claim. UniCore also puts Open Badges 3.0 and ELM 3.3 payloads into this envelope (see below).         | No                                                                | [OpenID4VCI 1.0, Appendix A.1.1][vci-jwt-vc-json]; [OpenID4VP 1.0, Appendix B.1.3.1][vp-jwt-vc-json]. Both reference VCDM 1.1.                                         | Issuance (all data models, see mapping). Holding: the only format the holder accepts. Verification.                                                                      |
| `vc+sd-jwt`       | `vc+sd-jwt`               | W3C VCDM 2.0: the credential is the JWT claims set, with `@context`, `type`, `issuer` and `credentialSubject` at the top level. No `vc` claim. | Yes. UniCore makes each `credentialSubject` property disclosable. | [W3C VC-JOSE-COSE §3.2.1 "Securing JSON-LD Verifiable Credentials with SD-JWT"][vcjc-sd-jwt]. **Not** a format profile of OpenID4VCI 1.0 or OpenID4VP 1.0 (see below). | Issuance (W3C VC 2.0, Open Badges 3.0, ELM 3.3 for individual holders). Verification.                                                                                    |
| `dc+sd-jwt`       | `dc+sd-jwt`               | IETF SD-JWT VC: flat JSON claims, type in `vct`, no `@context` or `credentialSubject`.                                                         | Yes. UniCore makes every non-registered claim disclosable.        | [IETF SD-JWT VC][sd-jwt-vc] on [RFC 9901 (SD-JWT)][rfc9901]; [OpenID4VCI 1.0, Appendix A.3][vci-sd-jwt-vc]; [OpenID4VP 1.0, Appendix B.3][vp-sd-jwt-vc].               | Verification. Issuance is implemented (signing, `vct`, Type Metadata at `/vct/{id}/{version}`), but no template data model maps to it yet, so templates cannot issue it. |

Not supported: `jwt_vc_json-ld`, `ldp_vc` (Data Integrity) and `mso_mdoc` (ISO mdoc). A verifier request that asks for `ldp_vc` or `mso_mdoc` fails when the response is validated.

### Template data model to format

| Template `dataModel`           | `holderType: individual` | `holderType: organization` |
| ------------------------------ | ------------------------ | -------------------------- |
| `w3c_vc_data_model_v1-1`       | `jwt_vc_json`            | `jwt_vc_json`              |
| `w3c_vc_data_model_v2-0`       | `vc+sd-jwt`              | `jwt_vc_json`              |
| `open_badges_3-0`              | `vc+sd-jwt`              | `jwt_vc_json`              |
| `european_learning_model_v3-3` | `vc+sd-jwt`              | `jwt_vc_json`              |

Organization holders get `jwt_vc_json` because the UniCore holder only stores JWT credentials.

The `@context` UniCore writes depends on the data model, not only on the format:

- W3C VC 1.1, and W3C VC 2.0 issued as `jwt_vc_json`: `https://www.w3.org/2018/credentials/v1`.
- W3C VC 2.0 issued as `vc+sd-jwt`: `https://www.w3.org/ns/credentials/v2`.
- Open Badges 3.0, in either format: `https://www.w3.org/ns/credentials/v2` followed by the Open Badges 3.0.3 context.
- ELM 3.3, in either format: both the v1 and the v2 context, v1 first, because the ELM schema still references VCDM 1.1.

## Things to know

- **`jwt_vc_json` means VCDM 1.1.** OpenID4VCI 1.0 and OpenID4VP 1.0 define `jwt_vc_json` against VCDM 1.1 (their `[VC_DATA]` reference). VCDM 2.0 moved JWT securing out of the data model into VC-JOSE-COSE ([VCDM 2.0 revision history][vcdm2-history]). VC-JOSE-COSE forbids the `vc` and `vp` claims and uses `typ` `vc+jwt` for plain JWTs. UniCore does not issue `vc+jwt`.
- **`vc+sd-jwt` has two meanings.** IETF SD-JWT VC used `typ` `vc+sd-jwt` from July 2023 until November 2024 (draft -06), then renamed it to `dc+sd-jwt` to avoid a conflict with the `vc` media type registered by W3C. Today `vc+sd-jwt` is the W3C media type for a VCDM 2.0 credential secured with SD-JWT ([VC-JOSE-COSE, media type registration][vcjc-media-type]). Older wallets can still send SD-JWT VCs with `typ` `vc+sd-jwt`. UniCore decides the format from the `typ` header and reads `vc+sd-jwt` as VCDM 2.0.
- **`vc+sd-jwt` is not an OpenID4VCI 1.0 format profile.** Appendix A of OpenID4VCI 1.0 defines `jwt_vc_json`, `jwt_vc_json-ld`, `ldp_vc`, `mso_mdoc` and `dc+sd-jwt`. It allows other specifications or deployments to define more profiles, and recommends using the media type as the identifier ([Appendix A][vci-profiles]). UniCore follows that rule: it advertises `vc+sd-jwt` with a `credential_definition.type` array. A wallet has to know this convention; no specification defines its parameters. OpenID4VP 1.0 does not define `vc+sd-jwt` either. Its own way to carry a W3C credential with SD-JWT is SD-JWT VCLD, a `dc+sd-jwt` credential with the W3C credential in an `ld` claim ([Appendix B.3.7][vp-sd-jwt-vcld]). UniCore does not implement SD-JWT VCLD.
- **SD-JWT VC is not a W3C verifiable credential.** VCDM 2.0 says so explicitly ([Ecosystem Compatibility][vcdm2-compat]). Its claims sit at the top level. `iss`, `nbf`, `exp`, `cnf`, `vct`, `vct#integrity` and `status` must not be selectively disclosable, while `sub` and `iat` may be ([SD-JWT VC, Registered JWT Claims][sd-jwt-vc]). Type Metadata, retrieved from the `vct` URL, describes display and per-claim metadata (path, display, mandatory, selective disclosure policy).
- **What Open Badges 3.0 requires.** Open Badges 3.0 (document version 1.2) uses VCDM 2.0: the first `@context` must be `https://www.w3.org/ns/credentials/v2`, followed by the Open Badges context. It defines two proof formats ([Proofs (Signatures)][ob-proofs]):
  - a JSON Web Token proof, where the JWT payload is the credential itself with `iss`, `jti`, `nbf` and `sub`, `typ` is `JWT` if present, and `alg` is at least `RS256`. A `vc` claim is only mentioned as something verifiers may meet in VCDM 1.1 credentials ([JWT proof][ob-jwt]);
  - a Linked Data (Data Integrity) proof. Conformance certification accepts `eddsa-rdfc-2022` and `ecdsa-sd-2023` ([Linked Data proof][ob-ldp], [issuer certification][ob-cert]).

  Open Badges 3.0 does not define an SD-JWT proof format. Neither of the formats UniCore uses for Open Badges today (`jwt_vc_json` with a `vc` claim, or `vc+sd-jwt`) is one of the two Open Badges proof formats.

## Profiles

- **HAIP 1.0** (OpenID4VC High Assurance Interoperability Profile, final). Each flow must support IETF SD-JWT VC (`dc+sd-jwt`) or ISO mdoc (`mso_mdoc`), or both; it does not list W3C formats ([§3 Scope][haip-scope], [§6][haip-profiles]). For SD-JWT VC it also requires issuer key resolution through an X.509 chain in the `x5c` header ([§6.1.1][haip-x5c]) and a JWK in `cnf` when holder binding is required ([§6.1][haip-sd-jwt-vc]). UniCore today signs with DID keys and sets `cnf` to a `kid`.
- **DIIP** (FIDES Decentralized Identity Interop Profile). Both the released v4 and the v6 draft require support for **both** SD-JWT VC **and** W3C VCDM 2.0 secured with SD-JWT as in VC-JOSE-COSE, plus ES256 and the IETF Token Status List ([DIIP, Credential Format][diip]). So for DIIP, `vc+sd-jwt` is required, not optional, even though OpenID4VCI 1.0 has no format profile for it. DIIP does not say which format identifier to use for it in OpenID4VCI. v4 references SD-JWT VC draft -08 and OpenID4VCI draft 15; the v6 draft references SD-JWT VC draft -13 and OpenID4VCI/OpenID4VP 1.0. The v6 draft also requires issuers to put a `fed` claim (SD-JWT VC) or a `termsOfUse` entry (VCDM) into credentials, as defined in its OpenID Federation profile. UniCore does not do this yet.

## Planned / under consideration

**Status: proposal, not decided.** These are candidates with a basis in the specifications above, not commitments.

- **Open Badges 3.0 conformance.** Issue Open Badges with one of the proof formats Open Badges 3.0 defines: the JWT proof (credential as the JWT payload, no `vc` claim) or a Data Integrity proof with `eddsa-rdfc-2022`. Today UniCore uses `jwt_vc_json` or `vc+sd-jwt`.
- **Format chosen by the wallet.** Today each template has exactly one credential configuration, with one format. OpenID4VCI lets the wallet pick among the issuer's `credential_configurations_supported`, so a template could be offered once per format, and the wallet's credential request would decide the format. Template authors would then only choose the data model.
- **ES256 by default for SD-JWT credentials.** DIIP requires ES256. UniCore signs every format with the one preferred algorithm, which is EdDSA by default, while the default verifier metadata lists only ES256 for both SD-JWT formats.
- **`dc+sd-jwt` from templates.** Map templates to `dc+sd-jwt`, as required by HAIP and DIIP. HAIP compliance would also need `x5c` issuer keys and `cnf.jwk`.
- **DIIP v6.** The `fed` and `termsOfUse` claims from the DIIP OpenID Federation profile.
- **ELM on VCDM 2.0.** VCDM 2.0 requires `https://www.w3.org/ns/credentials/v2` as the first `@context` ([Contexts][vcdm2-contexts]). ELM credentials put the v1 context first until the ELM schema moves to VCDM 2.0.
- **Not planned unless a use case needs them:** `ldp_vc`, `jwt_vc_json-ld`, `mso_mdoc`, `vc+jwt`, SD-JWT VCLD.

## Code references

- Template to format: `agent_issuance/src/application/credential_configuration_projection.rs` (`credential_configuration_from_template`)
- Signing per format, `typ` and `@context`: `agent_issuance/src/credential/aggregate.rs`
- Format detection of pre-signed credentials: `agent_shared/src/signed_credential_format.rs`
- Verifier request and default `vp_formats_supported`: `agent_api_http/src/v0/verification/authorization_requests.rs`, `agent_shared/src/config/mod.rs`
- Holder: `agent_holder/src/offer/aggregate.rs` (`SendCredentialRequest`)
- SD-JWT VC Type Metadata endpoint: `agent_api_http/src/v0/issuance/ietf_oauth_sd_jwt_vc.rs`

## Sources

- OpenID for Verifiable Credential Issuance 1.0 (Final, 16 September 2025): https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html, Appendix A
- OpenID for Verifiable Presentations 1.0 (Final, 9 July 2025): https://openid.net/specs/openid-4-verifiable-presentations-1_0.html, Appendix B
- OpenID4VC High Assurance Interoperability Profile 1.0 (Final, 24 December 2025): https://openid.net/specs/openid4vc-high-assurance-interoperability-profile-1_0.html, §3, §5.3, §6
- W3C Securing Verifiable Credentials using JOSE and COSE (Recommendation, 15 May 2025): https://www.w3.org/TR/vc-jose-cose/
- W3C Verifiable Credentials Data Model v2.0 (Recommendation, 15 May 2025): https://www.w3.org/TR/vc-data-model-2.0/
- W3C Verifiable Credentials Data Model v1.1: https://www.w3.org/TR/vc-data-model-1.1/
- IETF SD-JWT-based Verifiable Digital Credentials (SD-JWT VC), Internet-Draft: https://datatracker.ietf.org/doc/draft-ietf-oauth-sd-jwt-vc/
- RFC 9901, Selective Disclosure for JSON Web Tokens (November 2025): https://www.rfc-editor.org/rfc/rfc9901 (format §4, verification and processing §7)
- 1EdTech Open Badges 3.0 (document version 1.2): https://www.imsglobal.org/spec/ob/v3p0, certification guide: https://www.imsglobal.org/spec/ob/v3p0/cert
- FIDES DIIP: https://FIDEScommunity.github.io/DIIP/ (v4: `spec/spec.md` lines 31–32 and 121–123 at commit `14443cb`; v6 draft: lines 37–38, 134–136 and 158–160 at commit `f9f1df4`, and `spec/federation-profile.md` lines 135 and 160), repository https://github.com/FIDEScommunity/DIIP

[vci-profiles]: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-credential-format-profiles
[vci-jwt-vc-json]: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-vc-signed-as-a-jwt-not-usin
[vci-sd-jwt-vc]: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-ietf-sd-jwt-vc
[vp-jwt-vc-json]: https://openid.net/specs/openid-4-verifiable-presentations-1_0.html#name-vc-signed-as-a-jwt-not-usin
[vp-sd-jwt-vc]: https://openid.net/specs/openid-4-verifiable-presentations-1_0.html#name-ietf-sd-jwt-vc
[vp-sd-jwt-vcld]: https://openid.net/specs/openid-4-verifiable-presentations-1_0.html#name-sd-jwt-vcld
[haip-scope]: https://openid.net/specs/openid4vc-high-assurance-interoperability-profile-1_0.html#name-scope
[haip-profiles]: https://openid.net/specs/openid4vc-high-assurance-interoperability-profile-1_0.html#name-openid4vc-credential-format
[haip-sd-jwt-vc]: https://openid.net/specs/openid4vc-high-assurance-interoperability-profile-1_0.html#name-ietf-sd-jwt-vc-profile
[haip-x5c]: https://openid.net/specs/openid4vc-high-assurance-interoperability-profile-1_0.html#name-issuer-identification-and-k
[vcjc-sd-jwt]: https://www.w3.org/TR/vc-jose-cose/#secure-with-sd-jwt
[vcjc-media-type]: https://www.w3.org/TR/vc-jose-cose/#vc-sd-jwt-media-type
[vcdm2-contexts]: https://www.w3.org/TR/vc-data-model-2.0/#contexts
[vcdm2-compat]: https://www.w3.org/TR/vc-data-model-2.0/#ecosystem-compatibility
[vcdm2-history]: https://www.w3.org/TR/vc-data-model-2.0/#revision-history
[sd-jwt-vc]: https://datatracker.ietf.org/doc/draft-ietf-oauth-sd-jwt-vc/
[rfc9901]: https://www.rfc-editor.org/rfc/rfc9901
[ob-proofs]: https://www.imsglobal.org/spec/ob/v3p0#proofs-signatures
[ob-jwt]: https://www.imsglobal.org/spec/ob/v3p0#jwt-proof
[ob-ldp]: https://www.imsglobal.org/spec/ob/v3p0#lds-proof
[ob-cert]: https://www.imsglobal.org/spec/ob/v3p0/cert
[diip]: https://FIDEScommunity.github.io/DIIP/#credential-format
