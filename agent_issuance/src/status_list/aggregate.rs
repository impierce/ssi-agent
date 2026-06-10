#[cfg(feature = "test_utils")]
use agent_shared::config::TESTINDEX;
use agent_shared::config::{BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT};
use cqrs_es::Aggregate;
use oauth_tsl::status_list::{Bits, StatusList};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    services::IssuanceServices,
    status_list::{command::StatusListCommand, error::StatusListError, event::StatusListEvent},
};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StatusListAggregate {
    pub id: String,
    pub list: StatusList,
    pub used_indices: Vec<usize>,
}

impl Aggregate for StatusListAggregate {
    type Command = StatusListCommand;
    type Event = StatusListEvent;
    type Error = StatusListError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "status_list";

    async fn handle(&mut self, command: Self::Command, _services: &Self::Services, sink: &cqrs_es::event_sink::EventSink<Self>) -> Result<(), Self::Error> {
        use StatusListCommand::*;
        use StatusListEvent::*;

        let events: Vec<Self::Event> = match command {
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

                fill_status_list_with_random_values(&mut status_list, &used_indices)?;

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

                fill_status_list_with_random_values(&mut status_list, &self.used_indices)?;

                Ok(vec![IndexUpdated {
                    id: self.id.clone(),
                    status_list,
                    index,
                    status,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
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

// Helper

/// This function fills the remaining unused indices of a status list with random values to enhance privacy and security.
/// This block works in tandem with the part of `fn patch_credential` which only fills up to 70% of a status list, ensuring at least 30% randomness.
fn fill_status_list_with_random_values(
    status_list: &mut StatusList,
    used_indices: &[usize],
) -> Result<(), StatusListError> {
    use rand::Rng;

    let amount_indices = STATUS_LIST_BYTES_AMOUNT * 8 / BITS_PER_STATUS as usize;

    for i in 0..amount_indices {
        if !used_indices.contains(&i) {
            // rng must be initialized here, otherwise errors occur with axum due to thread unsafe problems and the Send trait
            let mut rng = rand::rng();
            // the range is 0..2 because BITS_PER_STATUS is set to 2, meaning 4 options, but we only have 3 options defined (VALID, UNVALID, SUSPENDED)
            status_list
                .set_status(i, rng.random_range(0..2))
                .map_err(StatusListError::StatusListEncodingError)?;
        }
    }

    Ok(())
}
