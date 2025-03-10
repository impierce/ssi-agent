use axum::{extract::State, Json};
use chrono::TimeDelta;
use serde::Serialize;

use super::MetadataState;

#[derive(Serialize)]
pub struct Info {
    version: String,
    git_commit: String,
    release_channel: String, // tbd: stable/latest (main), next, beta, canary (alpha)
    docker_build_timestamp: String,
    uptime: String,
}

/// A simple liveness probe following application monitoring conventions.
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
        version: std::env::var("UNICORE__APP_VERSION").unwrap_or_else(|_| "unknown".to_string()),
        git_commit: std::env::var("UNICORE__APP_GIT_COMMIT").unwrap_or_else(|_| "unknown".to_string()),
        release_channel: std::env::var("UNICORE__APP_RELEASE_CHANNEL").unwrap_or_else(|_| "unknown".to_string()),
        docker_build_timestamp: std::env::var("UNICORE__APP_DOCKER_BUILD_DATE")
            .unwrap_or_else(|_| "unknown".to_string()),
        uptime: uptime_human_readable,
    };
    Json(info)
}
