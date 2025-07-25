use agent_issuance::{credential::aggregate::CredentialStatus, state::IssuanceState};
use agent_shared::config::{config, get_preferred_did_method, get_preferred_signing_algorithm};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http::StatusCode;
use hyper::header;
use oauth_tsl::{
    managers::{relying_party::StatusListTokenResponseType, status_provider::compress_gzip},
    status_list::{Bits, EncodedStatusList, StatusList},
    tokens::status_list_token::{StatusListToken, StatusListTokenClaims},
};
use oid4vc_core::jwt::encode;
use rand::Rng;
use tokio::task;

use crate::{
    handlers::query_handler,
    issuance::{
        credentials::{STATUSLISTSIZE, STATUSTYPESIZE},
        error::PublicError,
    },
};

pub async fn token_status_list(
    State(state): State<IssuanceState>,
    Path(status_list_number): Path<usize>,
) -> Result<Response, PublicError> {
    let all_credentials = query_handler("all_credentials", &state.query.all_credentials)
        .await?
        .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    let lower_bound = status_list_number * STATUSLISTSIZE;
    let upper_bound = (status_list_number + 1) * STATUSLISTSIZE;

    let mut used_indices: Vec<CredentialStatus> = all_credentials
        .iter()
        .filter_map(|c| {
            let index = c.credential_status.index;
            if index >= lower_bound && index < upper_bound {
                Some(c.credential_status.clone())
            } else {
                None
            }
        })
        .collect();

    // This block ensures that the remaining empty 30% of a status list is filled with random values.
    // This block works in tandem with the part of `fn patch_credential` which only fills 70% of a status list.
    used_indices = task::spawn_blocking(move || -> Result<Vec<CredentialStatus>, PublicError> {
        let mut indices = used_indices.clone();

        let mut rng = rand::rng();
        while indices.len() < STATUSLISTSIZE {
            let random_index = rng.random_range(lower_bound..upper_bound);
            if !indices
                .iter()
                .any(|credential_status| credential_status.index == random_index)
            {
                let status_type = rng.random_range(0..2);
                indices.push(CredentialStatus {
                    index: random_index,
                    status: status_type.try_into().map_err(|_| PublicError::InternalServerError)?,
                });
            }
        }

        Ok(indices)
    })
    .await
    .map_err(|_| PublicError::InternalServerError)??;

    let status_list = StatusList {
        status_size: Bits::try_from(STATUSTYPESIZE).map_err(|_| PublicError::InternalServerError)?,
        status_list: used_indices
            .iter()
            .map(|credential_status| {
                credential_status
                    .status
                    .clone()
                    .try_into()
                    .map_err(|_| PublicError::InternalServerError)
            })
            .collect::<Result<Vec<_>, _>>()?,
        aggregation_uri: None,
    };

    let sub_uri = config()
        .ietf_oauth_token_status_list_uri
        .clone()
        .join(&format!("{status_list_number}"))
        .map_err(|_| PublicError::InternalServerError)?;
    let status_list_claims = StatusListTokenClaims {
        sub: sub_uri.to_string(),
        iat: chrono::Utc::now().timestamp(), // this is perhaps incorrect actually? check with spec if it wants first date of creation/usage of the TSL. wouldnt need to add some logic since we actually create on the fly.
        exp: None,
        ttl: None,
        encoded_status_list: EncodedStatusList::try_from(status_list).map_err(|_| PublicError::InternalServerError)?,
    };

    let mut status_list_token = StatusListToken {
        claims: status_list_claims,
        ..Default::default()
    };
    status_list_token.header.alg = get_preferred_signing_algorithm();
    let default_did_method =
        serde_json::to_string(&get_preferred_did_method()).map_err(|_| PublicError::InternalServerError)?;

    println!("here");
    // println!("signer: {:?}", state.signer);
    println!("status_list_token: {:?}", status_list_token);
    println!("default_did_method: {}", default_did_method);

    let jwt_token = encode(
        state.signer,
        status_list_token.header,
        status_list_token.claims,
        &default_did_method,
    )
    .await
    .unwrap();
    // .map_err(|_| PublicError::InternalServerError)?;

    println!("done");

    let compressed_jwt_token = compress_gzip(&jwt_token).map_err(|_| PublicError::InternalServerError)?;

    println!("compressed");

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, StatusListTokenResponseType::Jwt.as_str()),
            (header::CONTENT_ENCODING, "gzip"),
        ],
        compressed_jwt_token,
    )
        .into_response())
}

// #[cfg(test)]
// pub mod tests {
//     use super::*;

//     #[tokio::test]
//     pub async fn test_token_status_list() {}
// }
