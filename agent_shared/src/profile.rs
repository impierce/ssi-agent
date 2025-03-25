use std::env;
use std::str::FromStr;
use strum::{Display, EnumString};

#[derive(Debug, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ApplicationProfile {
    Production,
    Development,
}

impl ApplicationProfile {
    pub fn load() -> Self {
        env::var("UNICORE__PROFILE")
            .ok()
            .and_then(|profile_str| ApplicationProfile::from_str(&profile_str).ok())
            .unwrap_or(ApplicationProfile::Production)
    }
}

// impl Default for ApplicationProfile {
//     fn default() -> Self {
//         let profile_str = env::var("UNICORE__PROFILE").unwrap_or_else(|_| "production".to_string());
//         ApplicationProfile::from_str(&profile_str).unwrap_or(ApplicationProfile::Production)
//     }
// }
