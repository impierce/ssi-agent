use agent_issuance::{
    command::{IssuanceCommand, Metadata},
    state::new_application_state,
};
use cqrs_es::persist::ViewRepository;
use serde_json::json;

#[tokio::test]
async fn test() {
    pub fn credential_template() -> serde_json::Value {
        serde_json::from_str(include_str!("../resources/json_schema/openbadges_v3.json")).unwrap()
    }

    let application_state = new_application_state().await;

    let command = IssuanceCommand::LoadCredentialTemplate {
        credential_template: credential_template(),
    };

    dbg!(application_state
        .credential_query
        .load("agg-id-0006")
        .await
        .unwrap()
        .is_some());

    application_state
        .cqrs
        .execute("agg-id-0006", command)
        .await
        .unwrap();

    dbg!(application_state
        .credential_query
        .load("agg-id-0006")
        .await
        .unwrap()
        .is_some());

    let application_state = new_application_state().await;

    dbg!(application_state
        .credential_query
        .load("agg-id-0006")
        .await
        .unwrap()
        .is_some());

    let command = IssuanceCommand::CreateCredentialData {
        credential: serde_json::json!({
          "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
          ],
          "id": "http://example.com/credentials/3527",
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "id": "https://example.com/issuers/876543",
            "type": "Profile",
            "name": "Example Corp"
          },
          "issuanceDate": "2010-01-01T00:00:00Z",
          "name": "Teamwork Badge",
          "credentialSubject": {
            "id": "did:example:ebfeb1f712ebc6f1c276e12ec21",
            "type": "AchievementSubject",
            "achievement": {
                      "id": "https://example.com/achievements/21st-century-skills/teamwork",
                      "type": "Achievement",
                      "criteria": {
                          "narrative": "Team members are nominated for this badge by their peers and recognized upon review by Example Corp management."
                      },
                      "description": "This badge recognizes the development of the capacity to collaborate within a group environment.",
                      "name": "Teamwork"
                  }
          }
        }),
    };
    application_state
        .cqrs
        .execute("agg-id-0002", command)
        .await
        .unwrap();
}
