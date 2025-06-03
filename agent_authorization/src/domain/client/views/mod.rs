pub mod all_clients;

use super::aggregate::Client;
use cqrs_es::{EventEnvelope, View};

pub type ClientView = Client;

impl View<Client> for Client {
    fn update(&mut self, event: &EventEnvelope<Client>) {
        match &event.payload {
            _ => todo!(),
        }
    }
}
