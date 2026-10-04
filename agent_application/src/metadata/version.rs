use axum::Json;
use serde::Serialize;
use serde_with::skip_serializing_none;

include!(concat!(env!("OUT_DIR"), "/metadata.rs"));

#[skip_serializing_none]
#[derive(Serialize, utoipa::ToSchema)]
pub struct Version {
    /// The current version of the application.
    version: Option<String>,
    /// The git commit hash from which the application was built.
    git_commit_hash: Option<String>,
}

/// Returns the `version` and the `git_commit_hash` of the application.
#[utoipa::path(
    get,
    path = "/version",
    operation_id = "version",
    tags = ["Metadata"],
    responses(
        (status = 200, description = "Application version information", body = Version),
    )
)]
pub async fn version() -> Json<Version> {
    let version = version_inner();
    Json(version)
}

pub fn version_inner() -> Version {
    Version {
        version: APP_VERSION
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        git_commit_hash: GIT_COMMIT_HASH
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string().chars().take(7).collect()),
    }
}
