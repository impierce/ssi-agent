#[cfg(not(feature = "test_utils"))]
use agent_shared::config::BITS_PER_STATUS;
use agent_shared::config::STATUS_LIST_BYTES_AMOUNT;
#[cfg(feature = "test_utils")]
use agent_shared::config::TESTINDEX;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oauth_tsl::status_list::{Bits, StatusList};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    services::IssuanceServices,
    status_list::{command::StatusListCommand, error::StatusListError, event::StatusListEvent},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

        match command {
            CreateStatusList { id } => Ok(vec![StatusListCreated {
                id,
                status_list: StatusList {
                    status_size: Bits::Two, // must be equal to BITS_PER_STATUS
                    status_list: vec![0; STATUS_LIST_BYTES_AMOUNT],
                    aggregation_uri: None,
                },
                used_indices: vec![],
            }]),
            _ if self.id.is_empty() => return Err(StatusListError::AggregateNotFound),
            AddIndex { status } => {
                let mut status_list = self.list.clone();
                let mut used_indices = self.used_indices.clone();

                #[cfg(feature = "test_utils")]
                let index = TESTINDEX;

                #[cfg(not(feature = "test_utils"))]
                let index = {
                    use rand::Rng;

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
