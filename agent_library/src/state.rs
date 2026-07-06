use agent_shared::application_state::CommandHandler;
use shared_kernel::authorization::AuthorizationChecker;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;

use crate::template::{
    aggregate::Template,
    views::{all_templates::AllTemplatesView, TemplateView},
};

#[derive(Clone)]
pub struct LibraryState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub template: CommandHandler<Template>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries =
    ViewRepositories<dyn DynViewRepository<TemplateView, Template>, dyn DynViewRepository<AllTemplatesView, Template>>;

pub struct ViewRepositories<T1, T2>
where
    T1: DynViewRepository<TemplateView, Template> + ?Sized,
    T2: DynViewRepository<AllTemplatesView, Template> + ?Sized,
{
    pub template: Arc<T1>,
    pub all_templates: Arc<T2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            template: self.template.clone(),
            all_templates: self.all_templates.clone(),
        }
    }
}
