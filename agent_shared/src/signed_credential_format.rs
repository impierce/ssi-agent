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

pub fn detect_signed_credential_format(signed_credential: &str) -> Option<SignedCredentialFormat> {
    if signed_credential.contains('~') {
        let issuer_jwt = signed_credential.split('~').find(|segment| !segment.is_empty())?;

        let header = decode_jwt_segment_json(issuer_jwt, 0)?;
        match header.get("typ").and_then(Value::as_str) {
            Some("vc+sd-jwt") => Some(SignedCredentialFormat::VcSdJwt),
            Some("dc+sd-jwt") => Some(SignedCredentialFormat::DcSdJwt),
            _ => None,
        }
    } else {
        let payload = decode_jwt_segment_json(signed_credential, 1)?;
        if payload.get("vc").is_some() {
            Some(SignedCredentialFormat::JwtVcJson)
        } else {
            None
        }
    }
}

fn decode_jwt_segment_json(jwt: &str, segment_index: usize) -> Option<Value> {
    let segment = jwt.split('.').nth(segment_index)?;

    let decoded = URL_SAFE_NO_PAD.decode(segment).ok()?;

    serde_json::from_slice(&decoded).ok()
}
