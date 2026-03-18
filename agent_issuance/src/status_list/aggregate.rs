#[cfg(feature = "test_utils")]
use agent_shared::config::TESTINDEX;
use agent_shared::config::{BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oauth_tsl::status_list::{Bits, StatusList};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(not(feature = "test_utils"))]
use rand::Rng;

use crate::{
    services::IssuanceServices,
    status_list::{command::StatusListCommand, error::StatusListError, event::StatusListEvent},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusListAggregate {
    pub id: String,
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

        // TODO: I think all commands are currently Genesis commands while only CreateStatusList should be.
        // The other two will simply write over the Default value and store that, generating an incorrect StatusListAggregate.
        match command {
            CreateStatusList { id } => Ok(vec![StatusListCreated {
                id,
                status_list: StatusList {
                    status_size: Bits::Two,
                    status_list: vec![0; STATUS_LIST_BYTES_AMOUNT],
                    aggregation_uri: None,
                },
                used_indices: vec![],
            }]),
            AddIndex { status } => {
                let mut status_list = self.list.clone();
                let mut used_indices = self.used_indices.clone();

                #[cfg(feature = "test_utils")]
                let index = TESTINDEX;

                #[cfg(not(feature = "test_utils"))]
                let index = {
                    let mut rng = rand::rng();
                    let max_amount_indices = STATUS_LIST_BYTES_AMOUNT * (8 / BITS_PER_STATUS as usize);
                    loop {
                        let candidate = rng.random_range(0..max_amount_indices - 1);
                        if !self.used_indices.contains(&candidate) {
                            break candidate;
                        }
                    }
                };
                status_list
                    .set_status(index, status as u8)
                    .map_err(|e| StatusListError::FailedToSetIndex(index, e.to_string()))?;
                used_indices.push(index);

                Ok(vec![IndexAdded {
                    id: self.id.clone(),
                    status_list,
                    used_indices,
                    index,
                    status,
                }])
            }

            UpdateIndex { index, status } => {
                let mut status_list = self.list.clone();
                status_list
                    .set_status(index, status as u8)
                    .map_err(|e| StatusListError::FailedToSetIndex(index, e.to_string()))?;

                Ok(vec![IndexUpdated {
                    id: self.id.clone(),
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
            StatusListCreated {
                id,
                status_list,
                used_indices,
            } => {
                self.id = id;
                self.list = status_list;
                self.used_indices = used_indices;
            }
            IndexAdded {
                id,
                status_list,
                used_indices,
                index: _,
                status: _,
            } => {
                self.id = id;
                self.list = status_list;
                self.used_indices = used_indices;
            }
            IndexUpdated {
                id,
                status_list,
                index: _,
                status: _,
            } => {
                self.id = id;
                self.list = status_list;
            }
        }
    }
}

// TODO this whole default is not needed

// The Aggregate is initialized with the Default impl, in this case that means we need to default to the first Status List, ready to be used.
// We default the Status Size to 2 bits, the Status List amount of bytes is a const defined in agent_shared
impl Default for StatusListAggregate {
    // The default implementation is only for testing purposes and to satisfy the trait requirements for the Aggregate.
    fn default() -> Self {
        Self {
            id: "".to_string(),
            list: StatusList {
                status_size: Bits::Two, // equal to BITS_PER_STATUS
                status_list: vec![0; STATUS_LIST_BYTES_AMOUNT * 8 / BITS_PER_STATUS as usize], // This is were it becomes confusing between before and after packing the statuses from a Vec<u8>, holding one u8 per status, to a Vec<u8> holding the multipe statusses per byte as determined by the BITS_PER_STATUS const.
                aggregation_uri: None,
            },
            used_indices: vec![],
        }
    }
}
