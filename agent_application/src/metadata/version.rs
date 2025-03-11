use axum::Json;
use serde::Serialize;
use serde_with::skip_serializing_none;

include!(concat!(env!("OUT_DIR"), "/metadata.rs"));

#[skip_serializing_none]
#[derive(Serialize)]
pub struct Version {
    /// The current version of the application.
    version: Option<String>,
    /// The git commit hash from which the application was built.
    git_commit_hash: Option<String>,
}

/// Returns the `version` and the `git_commit_hash` of the application.
pub async fn version() -> Json<Version> {
    let version = Version {
        version: APP_VERSION.map(|s| s.to_string()),
        git_commit_hash: GIT_COMMIT_HASH.map(|s| s.to_string().chars().take(7).collect()),
    };
    Json(version)
}
