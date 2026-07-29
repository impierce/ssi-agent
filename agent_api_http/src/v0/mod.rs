pub mod authorization;
pub mod events;
pub mod holder;
pub mod identity;
pub mod issuance;
pub mod library;
pub mod openapi;
pub mod templates;
pub mod verification;

pub use crate::v0::events::openapi::EventsApi;
pub use crate::v0::holder::openapi::HolderApi;
pub use crate::v0::identity::connections::openapi::ConnectionsApi;
pub use crate::v0::identity::openapi::IdentityApi;
pub use crate::v0::issuance::openapi::IssuanceApi;
pub use crate::v0::library::catalog::openapi::CatalogsApi;
pub use crate::v0::templates::openapi::TemplatesApi;
