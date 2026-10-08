//! Smoke test: `run()` initializes telemetry, loads the subject, builds the application state and serves the HTTP API
//! on the port of the configured application URL.

use std::time::Duration;

#[tokio::test]
async fn run_serves_the_http_api_on_the_configured_port() {
    // A free port, since the default port `3033` may be taken by a locally running instance.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    // Each file in `tests/` runs in its own process, so this only affects the configuration of this test binary.
    std::env::set_var("UNICORE__PROFILE", "development");
    std::env::set_var("UNICORE__EVENT_STORE__TYPE", "in_memory");
    std::env::set_var("UNICORE__APPLICATION_URL", format!("http://localhost:{port}"));
    // Keep OpenTelemetry export disabled, regardless of the environment this test runs in.
    std::env::set_var("OTEL_SDK_DISABLED", "true");

    // `run()` is not `Send`, so, like `main`, it gets a runtime of its own. The thread ends with the test process.
    let server = std::thread::spawn(|| {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(agent_application::run())
    });

    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let response = loop {
        assert!(!server.is_finished(), "`run()` returned before serving requests");

        if let Ok(response) = client.get(format!("http://127.0.0.1:{port}/healthz")).send().await {
            break response;
        }

        assert!(
            tokio::time::Instant::now() < deadline,
            "the HTTP API was not served within 60 seconds"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    // `run()` verifies the persisted events before serving, which marks the application as ready.
    let response = client
        .get(format!("http://127.0.0.1:{port}/readyz"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    // CORS is enabled by the test configuration.
    let response = client
        .get(format!("http://127.0.0.1:{port}/version"))
        .header("Origin", "https://wallet.example.com")
        .send()
        .await
        .unwrap();
    assert!(response.headers().contains_key("access-control-allow-origin"));
}
