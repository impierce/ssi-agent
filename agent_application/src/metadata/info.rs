use axum::{extract::State, Json};
use chrono::TimeDelta;
use serde::Serialize;
use serde_with::skip_serializing_none;

use crate::metadata::version::{version_inner, Version};
use crate::metadata::MetadataState;

include!(concat!(env!("OUT_DIR"), "/metadata.rs"));

#[skip_serializing_none]
#[derive(Serialize)]
pub struct Info {
    #[serde(flatten)]
    version: Version,
    /// The release channel of the application. Possible values are: `stable`, `next`, `beta`, `canary`.
    release_channel: Option<String>, // TODO: mapping ok? stable/latest (main), next, beta, canary (alpha)
    /// The timestamp when the Docker image was built.
    docker_build_timestamp: Option<String>,
    /// The current uptime of the application in a human-friendly format.
    uptime: String,
}

/// Returns the `version`, application `uptime` among other build metadata.
pub async fn info(State(state): State<MetadataState>) -> Json<Info> {
    let time_delta = TimeDelta::seconds(state.startup_instant.elapsed().as_secs() as i64);
    let uptime_human_readable = format!(
        "{} days, {:02}:{:02}:{:02}",
        time_delta.num_days(),
        time_delta.num_hours() % 24,
        time_delta.num_minutes() % 60,
        time_delta.num_seconds() % 60
    );
    // Trim, filter out empty values, then convert to Option<String>.
    let info = Info {
        version: version_inner(),
        release_channel: APP_RELEASE_CHANNEL
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        docker_build_timestamp: DOCKER_BUILD_TIMESTAMP
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        uptime: uptime_human_readable,
    };
    Json(info)
}
