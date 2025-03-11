use axum::Json;
use serde::Serialize;
use serde_with::skip_serializing_none;

use crate::metadata::values::{APP_VERSION, GIT_COMMIT_HASH};

#[skip_serializing_none]
#[derive(Serialize)]
pub struct Version {
    version: Option<String>,
    git_commit_hash: Option<String>,
}

/// Returns the `version` and the `git_commit_hash` of the application.
pub async fn version() -> Json<Version> {
    let version = Version {
        version: APP_VERSION.map(|s| s.to_string()),
        git_commit_hash: GIT_COMMIT_HASH.map(|s| s.to_string()),
    };
    Json(version)
}
