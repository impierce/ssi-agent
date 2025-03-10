use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct Version {
    version: String,
    git_commit_hash: String,
}

/// Returns the `version` and the `git_commit_hash` of the application.
pub async fn version() -> Json<Version> {
    let version = Version {
        version: std::env::var("UNICORE__APP_VERSION").unwrap_or_else(|_| "unknown".to_string()),
        git_commit_hash: std::env::var("UNICORE__APP_GIT_COMMIT_HASH").unwrap_or_else(|_| "unknown".to_string()),
    };
    Json(version)
}
