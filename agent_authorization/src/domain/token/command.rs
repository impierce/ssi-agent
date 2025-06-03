use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TokenCommand {}
