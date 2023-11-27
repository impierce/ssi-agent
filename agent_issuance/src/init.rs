use crate::{
    command::IssuanceCommand, handlers::command_handler, model::aggregate::IssuanceData, queries::IssuanceDataView,
    state::DynApplicationState,
};
use tracing::info;

pub async fn load_templates(state: &DynApplicationState<IssuanceData, IssuanceDataView>) {
    match command_handler(
        "agg-id-F39A0C".to_string(),
        state,
        IssuanceCommand::LoadCredentialFormatTemplate {
            credential_format_template: serde_json::from_str(include_str!(
                "../res/credential_format_templates/openbadges_v3.json"
            ))
            .unwrap(),
        },
    )
    .await
    {
        Ok(_) => info!("Template loaded: \"Open Badges 3.0\""),
        Err(err) => println!("Template failed to load: {:#?}", err),
    };
}
