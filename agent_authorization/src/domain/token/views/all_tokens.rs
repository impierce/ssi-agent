use super::Token;
use super::TokenView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllTokensView {
    #[serde(flatten)]
    pub tokens: HashMap<String, TokenView>,
}

impl View<Token> for AllTokensView {
    fn update(&mut self, event: &EventEnvelope<Token>) {
        self.tokens
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
