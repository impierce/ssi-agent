use axum::Json;
use serde::Serialize;
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Serialize)]
pub struct Version {
    version: Option<String>,
    git_commit_hash: Option<String>,
}

/// Returns the `version` and the `git_commit_hash` of the application.
pub async fn version() -> Json<Version> {
    let version = Version {
        version: std::env::var("APP_VERSION").ok(),
        git_commit_hash: std::env::var("GIT_COMMIT_HASH").ok(),
    };
    Json(version)
}
