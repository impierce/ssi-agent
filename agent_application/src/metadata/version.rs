use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct Version {
    version: String,
    git_commit: String,
    // release_channel: String,
    // docker_build_timestamp: String,
}

/// A simple liveness probe following application monitoring conventions.
pub async fn version() -> Json<Version> {
    let version = Version {
        version: std::env::var("UNICORE__APP_VERSION").unwrap_or_else(|_| "unknown".to_string()),
        git_commit: std::env::var("UNICORE__APP_GIT_COMMIT").unwrap_or_else(|_| "unknown".to_string()),
        // release_channel: std::env::var("UNICORE__APP_RELEASE_CHANNEL").unwrap_or_else(|_| "unknown".to_string()),
        // docker_build_timestamp: std::env::var("UNICORE__APP_DOCKER_BUILD_DATE")
        //     .unwrap_or_else(|_| "unknown".to_string()),
    };
    Json(version)
}
