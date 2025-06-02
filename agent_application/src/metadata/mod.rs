pub mod config;
pub mod info;
pub mod version;

#[derive(Clone)]
pub struct MetadataState {
    pub startup_instant: std::time::Instant,
}
