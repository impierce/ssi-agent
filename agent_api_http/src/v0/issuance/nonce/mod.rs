#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonceEndpointRequest {}

#[axum_macros::debug_handler]
pub(crate) async fn nonce(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let c_nonce = generate_random_string();

    let command = NonceCommand::GenerateNonce { nonce: c_nonce.clone() };

    command_handler(&state, &state.command.nonce, command).await?;

    Ok((StatusCode::OK, Json(json!({ "c_nonce": c_nonce }))).into_response())
}
