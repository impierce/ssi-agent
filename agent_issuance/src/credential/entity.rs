use derivative::Derivative;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Data {
    #[derivative(PartialEq = "ignore")]
    pub id: uuid::Uuid,
    pub raw: serde_json::Value,
}
