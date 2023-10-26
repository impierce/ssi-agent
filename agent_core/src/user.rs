use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum User {
    Basic(Basic),
    Jwt(Jwt),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Basic {
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Jwt {
    pub sub: String,
}
