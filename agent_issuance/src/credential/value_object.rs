use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Subject {
    pub pre_authorized_code: String,
}

// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
// pub struct Format {}
