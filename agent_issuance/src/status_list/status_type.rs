use serde::{Deserialize, Serialize};

/// The status of a credential in a status list, as defined by the IETF OAuth Token Status List specification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StatusType {
    #[default]
    Valid,
    Invalid,
    Suspended,
    /// Application-specific status (`0x03`).
    Undefined,
}

impl From<StatusType> for identity_kit::sd_jwt_vc::StatusType {
    fn from(status: StatusType) -> Self {
        match status {
            StatusType::Valid => Self::VALID,
            StatusType::Invalid => Self::INVALID,
            StatusType::Suspended => Self::SUSPENDED,
            StatusType::Undefined => Self(3),
        }
    }
}
