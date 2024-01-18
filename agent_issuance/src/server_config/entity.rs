use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
// TODO: rename
pub struct Root {
    id: uuid::Uuid,
}

// TODO: use a more generic way of handling image assets in the agent (also for credential images)
pub struct Image {
    id: uuid::Uuid,
}
