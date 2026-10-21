pub struct KeyProviderServices {
    /// Whether development-only providers, such as the in-memory provider, may be added.
    pub development_providers_allowed: bool,
}

impl KeyProviderServices {
    #[must_use]
    pub fn new(development_providers_allowed: bool) -> Self {
        Self {
            development_providers_allowed,
        }
    }
}
