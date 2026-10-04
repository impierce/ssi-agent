/// State/services for public offer operations
/// This is currently a unit struct but can be expanded to hold
/// injected services if needed in the future.
#[derive(Clone, Debug)]
pub struct PublicOfferState;

impl PublicOfferState {
    pub fn new() -> Self {
        PublicOfferState
    }
}

impl Default for PublicOfferState {
    fn default() -> Self {
        Self::new()
    }
}
