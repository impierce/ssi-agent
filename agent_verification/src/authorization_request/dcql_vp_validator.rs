use crate::authorization_request::error::AuthorizationRequestError;
use oid4vp::dcql::dcql_query::DcqlQuery;
use oid4vp::token::vp_token::VpToken;
use oid4vp::token::vp_token_builder::VpTokenBuilder;

pub fn validate_vp_token_against_dcql_query(
    vp_token: &VpToken,
    dcql_query: &DcqlQuery,
) -> Result<(), AuthorizationRequestError> {
    // 1. validate the DCQL query itself
    // dcql_query
    //     .validate()
    //     .map_err(|e| AuthorizationRequestError::InvalidDcqlQuery(e))?;

    //then create a temporary builder to use its validation logic
    let builder = VpTokenBuilder::builder_dcql_query(dcql_query.clone());

    // add presentations from the received VP token
    let mut temp_builder = builder;
    for (credential_id, presentations) in vp_token.presentations() {
        for presentation in presentations {
            temp_builder = temp_builder.add_presentation(credential_id.clone(), presentation.clone());
        }
    }
    // makes use of the builders validation logic
    temp_builder.build().unwrap();
    // .map_err(|e| AuthorizationRequestError::VpTokenValidationFailed(e))?;

    Ok(())
}
