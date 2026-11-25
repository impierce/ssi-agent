use agent_shared::application_state::{CommandHandler, CqrsComponentBuilder};
use cqrs_es::Query;
use shared_kernel::authorization::{AllowAllAuthorizationChecker, AuthorizationChecker, QueryOperation};
use shared_kernel::event_bus::EventBusHandle;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;

use crate::catalog::services::{CatalogServiceImpl, CatalogServices};
use crate::template::{
    aggregate::Template,
    views::{all_templates::AllTemplatesView, TemplateView},
};

use crate::catalog::{
    aggregate::Catalog,
    views::{view_all_catalogs::AllCatalogsView, CatalogView},
};

impl QueryOperation for TemplateView {
    const OPERATION_NAME: &'static str = "library.templates.get";
}

impl QueryOperation for AllTemplatesView {
    const OPERATION_NAME: &'static str = "library.templates.list";
}

impl QueryOperation for CatalogView {
    const OPERATION_NAME: &'static str = "library.catalogs.get";
}

impl QueryOperation for AllCatalogsView {
    const OPERATION_NAME: &'static str = "library.catalogs.list";
}

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
    pub catalog: CommandHandler<Catalog>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn DynViewRepository<TemplateView, Template>,
    dyn DynViewRepository<AllTemplatesView, Template>,
    dyn DynViewRepository<CatalogView, Catalog>,
    dyn DynViewRepository<AllCatalogsView, Catalog>,
>;

pub struct ViewRepositories<T1, T2, T3, T4>
where
    T1: DynViewRepository<TemplateView, Template> + ?Sized,
    T2: DynViewRepository<AllTemplatesView, Template> + ?Sized,
    T3: DynViewRepository<CatalogView, Catalog> + ?Sized,
    T4: DynViewRepository<AllCatalogsView, Catalog> + ?Sized,
{
    pub template: Arc<T1>,
    pub all_templates: Arc<T2>,
    pub catalog: Arc<T3>,
    pub all_catalogs: Arc<T4>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            template: self.template.clone(),
            all_templates: self.all_templates.clone(),
            catalog: self.catalog.clone(),
            all_catalogs: self.all_catalogs.clone(),
        }
    }
}

/// Constructs the CQRS components and initializes the state for the Library bounded context.
///
/// Registers command handlers and view repositories for credential templates and catalogs
/// using the provided [`CqrsComponentBuilder`]. Custom template queries can be supplied
/// via `template_queries` to be executed alongside standard query projections.
pub async fn library_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    event_bus: &EventBusHandle,
    template_queries: Vec<Box<dyn Query<Template>>>,
) -> LibraryState {
    let mut queries: Vec<Box<dyn Query<Template>>> = vec![event_bus.query()];
    queries.extend(template_queries);

    let (template_command_handler, template, all_templates) = builder
        .commands_and_queries::<Template, Template, AllTemplatesView>((), queries)
        .await;

    let catalog_services: Arc<dyn CatalogServices> = Arc::new(CatalogServiceImpl {
        template_view_repo: template.clone(),
    });

    let (catalog_command_handler, catalog, all_catalogs) = builder
        .commands_and_queries::<CatalogView, Catalog, AllCatalogsView>(catalog_services, vec![event_bus.query()])
        .await;

    LibraryState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: CommandHandlers {
            template: template_command_handler,
            catalog: catalog_command_handler,
        },
        query: ViewRepositories {
            template,
            all_templates,
            catalog,
            all_catalogs,
        },
    }
}
