use std::str::FromStr as _;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use identity_iota::{
    core::{FromJson as _, ToJson as _},
    verification::{
        jwk::Jwk,
        jws::{JwsVerifier, SignatureVerificationError, VerificationInput},
    },
};
use jsonwebtoken::{crypto::verify, Algorithm, DecodingKey, Validation};

/// This `Verifier` uses `jsonwebtoken` under the hood to verify verification input.
pub struct Verifier;
impl JwsVerifier for Verifier {
    fn verify(&self, input: VerificationInput, public_key: &Jwk) -> Result<(), SignatureVerificationError> {
        let algorithm = Algorithm::from_str(&input.alg.to_string()).unwrap();

        println!("public_key: {:?}", public_key);

        // Convert the `Jwk` first into a `jsonwebtoken::jwk::Jwk` and then into a `DecodingKey`.
        let decoding_key = public_key
            .to_json()
            .ok()
            .and_then(|public_key| jsonwebtoken::jwk::Jwk::from_json(&public_key).ok())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
            .unwrap();

        let mut validation = Validation::new(algorithm);
        validation.validate_aud = false;
        validation.required_spec_claims.clear();

        println!("validation: {:?}", validation);

        verify(
            &URL_SAFE_NO_PAD.encode(input.decoded_signature),
            &input.signing_input,
            &decoding_key,
            algorithm,
        )
        .unwrap();

        Ok(())
    }
}
