use crate::nonce::aggregate::Nonce;
use cqrs_es::View;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NonceView {
    pub c_nonce: String,
    pub is_redeemed: bool,
}

impl View<Nonce> for NonceView {
    fn update(&mut self, event: &cqrs_es::EventEnvelope<Nonce>) {
        use crate::nonce::event::NonceEvent::*;

        match &event.payload {
            NonceGenerated { c_nonce, is_redeemed } => {
                self.c_nonce = c_nonce.clone();
                self.is_redeemed = *is_redeemed;
            }
            NonceRedeemed { is_redeemed, .. } => {
                self.is_redeemed = *is_redeemed;
            }
        }
    }
}
