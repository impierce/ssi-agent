use std::str::FromStr as _;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use identity_iota::core::{FromJson as _, ToJson as _};
use identity_iota::verification;
use identity_iota::verification::jws::{
    JwsVerifier, SignatureVerificationError, SignatureVerificationErrorKind, VerificationInput,
};
use jsonwebtoken::crypto::verify;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};

/// This `Verifier` uses `jsonwebtoken` under the hood to verify verification input.
pub struct Verifier;
impl JwsVerifier for Verifier {
    fn verify(
        &self,
        input: VerificationInput,
        public_key: &verification::jwk::Jwk,
    ) -> Result<(), SignatureVerificationError> {
        use SignatureVerificationErrorKind::*;

        let algorithm =
            Algorithm::from_str(&input.alg.to_string()).map_err(|_| SignatureVerificationError::new(UnsupportedAlg))?;

        // Convert the `IotaIdentityJwk` first into a `jsonwebtoken::Jwk` and then into a `DecodingKey`.
        let decoding_key = public_key
            .to_json()
            .ok()
            .and_then(|public_key| jsonwebtoken::jwk::Jwk::from_json(&public_key).ok())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
            .ok_or(SignatureVerificationError::new(KeyDecodingFailure))?;

        let mut validation = Validation::new(algorithm);
        validation.validate_aud = false;
        validation.required_spec_claims.clear();

        match verify(
            &URL_SAFE_NO_PAD.encode(input.decoded_signature),
            &input.signing_input,
            &decoding_key,
            algorithm,
        ) {
            Ok(_) => Ok(()),
            Err(_) => Err(SignatureVerificationError::new(
                // TODO: more fine-grained error handling?
                InvalidSignature,
            )),
        }
    }
}
