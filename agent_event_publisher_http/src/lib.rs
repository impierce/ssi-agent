use agent_shared::config;
use serde_json::json;

async fn publish_event() -> Result<(), Box<dyn std::error::Error>> {
    let target_url = config!("target_url").unwrap();

    println!("target_url: {}", target_url);

    let client = reqwest::Client::new();
    let resp = client
        .post(target_url)
        .json(&json!({
            "key": "value",
        }))
        .send()
        .await?;
    println!("{:#?}", resp);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn it_works() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ssi-events-subscriber"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let target_url = format!("http://{}/ssi-events-subscriber", &mock_server.address().to_string());

        std::env::set_var("TEST_TARGET_URL", &target_url); // which one needs to be set?
        std::env::set_var("AGENT_EVENT_PUBLISHER_HTTP_TARGET_URL", &target_url); // which one needs to be set?

        assert!(publish_event().await.is_ok());
    }
}
