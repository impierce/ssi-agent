use async_trait::async_trait;
use cqrs_es::Aggregate;
use oauth_tsl::status_list::StatusList;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use url::Url;

use crate::{services::IssuanceServices, status_list::{command::StatusListCommand, error::StatusListError, event::StatusListEvent}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusListAggregate {
    pub id: Url, // Since the referenced token in a credential also only has the URL as a unique identifier, which is fixed. Aligning these a 100% is therefore best.
    pub list: StatusList, // Hashmap<usize, StatusType>
    pub index: usize, 
    pub size: usize, 
    pub full: bool, // This bool will cut down computation time greatly to find the right Status List to add a new index to.
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
                // how to add the first index when there is nothing in self or to query?
                // query all status lists and find the one that is not full.
                // then find a random index that has not been used yet in that list
                // then add it
                let index = self.list.add_status(status);

                Ok(vec![IndexAdded { id: self.id.clone(), index, status }])
            }

            UpdateIndex { id, index, status } => {
                // query the status list by id
                // update the index
                self.list.update_status(index, status);
                Ok(vec![IndexUpdated { id, index, status }])
            }

            CreateToken { id } => {
                Ok(vec![TokenCreated { id }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use StatusListEvent::*;

        match event {
            IndexAdded { id: _, index, status } => {
                self.list.add_status(status);
            }

            IndexUpdated { id: _, index, status } => {
                self.list.update_status(index, status);
            }

            TokenCreated { id: _ } => {
                // No state change needed for token creation in the aggregate
            }
        }
    }

}

impl Default for StatusListAggregate {
    // The default implementation is only for testing purposes and to satisfy the trait requirements for the Aggregate.
    fn default() -> Self {
        Self {
            id: Url::parse("http://example.com/status-list").unwrap(), // Default URL, never use this except for testing,should be replaced with a proper one
            list: StatusList::default(),
            full: false, 
        }
    }
}