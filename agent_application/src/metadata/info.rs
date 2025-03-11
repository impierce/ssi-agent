use axum::{extract::State, Json};
use chrono::TimeDelta;
use serde::Serialize;
use serde_with::skip_serializing_none;

use crate::metadata::MetadataState;

include!(concat!(env!("OUT_DIR"), "/metadata.rs"));

#[skip_serializing_none]
#[derive(Serialize)]
pub struct Info {
    version: Option<String>,
    git_commit_hash: Option<String>,
    release_channel: Option<String>, // TODO: stable/latest (main), next, beta, canary (alpha)
    docker_build_timestamp: Option<String>,
    uptime: String,
}

/// Returns the `version`, application uptime and a few more metadata values.
pub async fn info(State(state): State<MetadataState>) -> Json<Info> {
    let time_delta = TimeDelta::seconds(state.startup_instant.elapsed().as_secs() as i64);
    let uptime_human_readable = format!(
        "{} days, {:02}:{:02}:{:02}",
        time_delta.num_days(),
        time_delta.num_hours() % 24,
        time_delta.num_minutes() % 60,
        time_delta.num_seconds() % 60
    );
    let info = Info {
        version: APP_VERSION.map(|s| s.to_string()),
        git_commit_hash: GIT_COMMIT_HASH.map(|s| s.to_string().chars().take(7).collect()),
        release_channel: std::env::var("UNICORE__APP_RELEASE_CHANNEL").ok(),
        docker_build_timestamp: DOCKER_BUILD_TIMESTAMP.map(|s| s.to_string()),
        uptime: uptime_human_readable,
    };
    Json(info)
}
