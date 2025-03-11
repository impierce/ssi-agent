pub mod info;
pub mod values;
pub mod version;

#[derive(Clone)]
pub struct MetadataState {
    pub startup_instant: std::time::Instant,
}
