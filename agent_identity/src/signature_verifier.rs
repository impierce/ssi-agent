use identity_iota::verification::{
    jwk::Jwk,
    jws::{JwsVerifier, SignatureVerificationError, SignatureVerificationErrorKind, VerificationInput},
};
use identity_kit::jose::{DefaultVerifier, JwsAlgorithm, JwsVerifier as _, VerificationError};

/// Verifies JWS signatures for the `identity_iota` validators with IdentityKit's [`DefaultVerifier`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SignatureVerifier;

impl JwsVerifier for SignatureVerifier {
    fn verify(&self, input: VerificationInput, public_key: &Jwk) -> Result<(), SignatureVerificationError> {
        let alg: JwsAlgorithm = serde_json::to_value(input.alg)
            .and_then(serde_json::from_value)
            .map_err(|_| SignatureVerificationErrorKind::UnsupportedAlg)?;
        let public_key: identity_kit::jose::Jwk = serde_json::to_value(public_key)
            .and_then(serde_json::from_value)
            .map_err(|_| SignatureVerificationErrorKind::KeyDecodingFailure)?;

        DefaultVerifier
            .verify(alg, &input.signing_input, &input.decoded_signature, &public_key)
            .map_err(|error| {
                let kind = match error {
                    VerificationError::UnsupportedAlgorithm(_) => SignatureVerificationErrorKind::UnsupportedAlg,
                    VerificationError::Key(_) => SignatureVerificationErrorKind::KeyDecodingFailure,
                    VerificationError::InvalidSignature => SignatureVerificationErrorKind::InvalidSignature,
                    _ => SignatureVerificationErrorKind::Unspecified,
                };
                SignatureVerificationError::new(kind).with_source(error)
            })
    }
}
