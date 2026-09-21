use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedCredentialFormat {
    JwtVcJson,
    VcSdJwt,
    DcSdJwt,
}

impl SignedCredentialFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            SignedCredentialFormat::JwtVcJson => "jwt_vc_json",
            SignedCredentialFormat::VcSdJwt => "vc+sd-jwt",
            SignedCredentialFormat::DcSdJwt => "dc+sd-jwt",
        }
    }
}

pub fn detect_signed_credential_format(signed_credential: &str) -> Result<SignedCredentialFormat> {
    if signed_credential.contains('~') {
        let issuer_jwt = signed_credential
            .split('~')
            .find(|segment| !segment.is_empty())
            .ok_or_else(|| anyhow!("Signed SD-JWT credential is missing the issuer JWT."))?;

        let header = decode_jwt_segment_json(issuer_jwt, 0)?;
        match header.get("typ").and_then(Value::as_str) {
            Some("vc+sd-jwt") => Ok(SignedCredentialFormat::VcSdJwt),
            Some("dc+sd-jwt") => Ok(SignedCredentialFormat::DcSdJwt),
            _ => Err(anyhow!(
                "Signed SD-JWT credential must declare header typ `vc+sd-jwt` or `dc+sd-jwt`."
            )),
        }
    } else {
        let payload = decode_jwt_segment_json(signed_credential, 1)?;
        if payload.get("vc").is_some() {
            Ok(SignedCredentialFormat::JwtVcJson)
        } else {
            Err(anyhow!(
                "Signed JWT credential must contain a `vc` claim to match `jwt_vc_json`."
            ))
        }
    }
}

fn decode_jwt_segment_json(jwt: &str, segment_index: usize) -> Result<Value> {
    let segment = jwt
        .split('.')
        .nth(segment_index)
        .ok_or_else(|| anyhow!("Signed credential is not a valid JWT."))?;

    let decoded = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| anyhow!("Signed credential contains invalid base64url data."))?;

    serde_json::from_slice(&decoded)
        .map_err(|_| anyhow!("Signed credential contains invalid JSON in its JWT segments."))
}
