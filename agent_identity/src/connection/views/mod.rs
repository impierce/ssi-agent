pub mod all_connections;

use super::event::ConnectionEvent;
use crate::connection::aggregate::Connection;
use cqrs_es::{EventEnvelope, View};

pub type ConnectionView = Connection;

impl View<Connection> for Connection {
    fn update(&mut self, event: &EventEnvelope<Connection>) {
        use ConnectionEvent::*;

        match &event.payload {
            ConnectionAdded {
                connection_id,
                domain,
                dids,
                credential_offer_endpoint,
            } => {
                self.connection_id.clone_from(connection_id);
                self.domain.clone_from(domain);
                self.dids.clone_from(dids);
                self.credential_offer_endpoint.clone_from(credential_offer_endpoint);
            }
        }
    }
}
