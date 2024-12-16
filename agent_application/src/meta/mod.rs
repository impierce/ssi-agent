pub mod info;
pub mod version;

#[derive(Clone)]
pub struct MetaState {
    pub startup_instant: std::time::Instant,
}
