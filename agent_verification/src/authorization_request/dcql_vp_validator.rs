use crate::authorization_request::error::AuthorizationRequestError;
use oid4vp::dcql::dcql_query::DcqlQuery;
use oid4vp::token::vp_token::VpToken;
use oid4vp::token::vp_token_builder::VpTokenBuilder;

pub fn validate_vp_token_against_dcql_query(
    vp_token: &VpToken,
    dcql_query: &DcqlQuery,
) -> Result<(), AuthorizationRequestError> {
    // Create a temporary builder to validate the VP token against the DCQL query
    let builder = VpTokenBuilder::builder_dcql_query(dcql_query.clone());

    // Add presentations from the received VP token
    let mut temp_builder = builder;
    for (credential_id, presentations) in vp_token.presentations() {
        for presentation in presentations {
            temp_builder = temp_builder.add_presentation(credential_id.clone(), presentation.clone());
        }
    }
    temp_builder.build().map_err(|_| {
        AuthorizationRequestError::VpTokenValidationFailed(anyhow::anyhow!(
            "VpToken validation failed against DCQL query"
        ))
    })?;

    Ok(())
}
