pub mod all_tokens;

use super::aggregate::Token;
use cqrs_es::{EventEnvelope, View};

pub type TokenView = Token;

impl View<Token> for Token {
    fn update(&mut self, event: &EventEnvelope<Token>) {
        match &event.payload {
            _ => todo!(),
        }
    }
}
