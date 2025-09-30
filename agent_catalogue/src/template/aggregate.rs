use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::{command::TemplateCommand, error::TemplateError, event::TemplateEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Template {
    #[serde(rename = "id")]
    pub template_id: String,
}

#[async_trait]
impl Aggregate for Template {
    type Command = TemplateCommand;
    type Event = TemplateEvent;
    type Error = TemplateError;
    type Services = ();

    fn aggregate_type() -> String {
        "template".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use TemplateCommand::*;
        use TemplateEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateTemplate { template_id } => Ok(vec![TemplateCreated { template_id }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use TemplateEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            TemplateCreated { template_id } => {
                self.template_id = template_id;
            }
        }
    }
}

#[cfg(test)]
pub mod document_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type TemplateTestFramework = TestFramework<Template>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_template(template_id: String) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id: template_id.clone(),
            })
            .then_expect_events(vec![TemplateEvent::TemplateCreated { template_id }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use rstest::fixture;

    #[fixture]
    pub fn template_id() -> String {
        "template_id".to_string()
    }
}
