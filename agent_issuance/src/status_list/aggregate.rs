use agent_shared::{
    config::{config, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT},
    generate_random_string,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oauth_tsl::status_list::{Bits, StatusList};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use url::Url;

use crate::{
    services::IssuanceServices,
    status_list::{command::StatusListCommand, error::StatusListError, event::StatusListEvent},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusListAggregate {
    pub id: Url, // Since the referenced token in a credential also only has the URL as a unique identifier, which is fixed. Aligning these a 100% is therefore best.
    pub list: StatusList,
    pub used_indices: Vec<usize>,
}

#[async_trait]
impl Aggregate for StatusListAggregate {
    type Command = StatusListCommand;
    type Event = StatusListEvent;
    type Error = StatusListError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "status_list".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use StatusListCommand::*;
        use StatusListEvent::*;

        match command {
            AddIndex { status } => {
                let max_amount_indices = STATUS_LIST_BYTES_AMOUNT * BITS_PER_STATUS as usize;
                let full = self.used_indices.len() >= max_amount_indices;

                // We only add a new status list if the current one is full.
                // Once we implement being able to use multiple status lists by specifying an ID in the command, this logic will need to be updated.
                if !full {
                    let mut status_list = self.list.clone();
                    let mut used_indices = self.used_indices.clone();

                    let mut rng = rand::rng();
                    let index = loop {
                        let candidate = rng.random_range(0..max_amount_indices - 1);
                        if !used_indices.contains(&candidate) {
                            break candidate;
                        }
                    };

                    status_list.set_status(index, status as u8);
                    used_indices.push(index);

                    Ok(vec![IndexAdded {
                        id: self.id,
                        status_list,
                        index,
                        status,
                        used_indices,
                    }])
                } else {
                    // Create a new Status List with a new ID
                    let mut new_aggregate = StatusListAggregate::default();

                    let mut rng = rand::rng();
                    let index = rng.random_range(0..max_amount_indices - 1);

                    new_aggregate.list.set_status(index, status as u8);
                    new_aggregate.used_indices.push(index);

                    Ok(vec![IndexAdded {
                        id: new_aggregate.id,
                        status_list: new_aggregate.list,
                        index,
                        status,
                        used_indices: new_aggregate.used_indices,
                    }])
                }
            }
            UpdateIndex { id, index, status } => {
                // query the status list by id
                // update the index
                Ok(vec![IndexUpdated {
                    id,
                    status_list,
                    index,
                    status,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use StatusListEvent::*;

        match event {
            IndexAdded {
                id,
                status_list,
                index: _,
                status: _,
                full,
            } => {
                self.id = id;
                self.list = status_list;
                self.full = full;
                // self.index = index;
                // self.status = status;
            }
            IndexUpdated {
                id,
                status_list,
                index: _,
                status: _,
            } => {
                self.id = id;
                self.list = status_list;
                // self.index = index;
                // self.status = status;
            }
        }
    }
}

// The Aggregate is initialized with the Default impl, in this case that means we need to default to the first Status List, ready to be used.
// We default the Status Size to 2 bits, the Status List amount of bytes is a const defined in agent_shared
impl Default for StatusListAggregate {
    // The default implementation is only for testing purposes and to satisfy the trait requirements for the Aggregate.
    fn default() -> Self {
        let status_list_id = generate_random_string();
        let mut status_list_url = config().ietf_oauth_token_status_list_uri.clone();
        status_list_url
            .path_segments_mut()
            .expect(&format!(
                "Failed to create default Status List ID due to an invalid URL: {}",
                config().ietf_oauth_token_status_list_uri.clone()
            ))
            .push(&status_list_id.to_string());

        Self {
            id: status_list_url,
            list: StatusList {
                status_size: Bits::Two,
                status_list: vec![0; STATUS_LIST_BYTES_AMOUNT],
                aggregation_uri: None,
            },
            full: false,
        }
    }
}
