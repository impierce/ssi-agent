use crate::template::aggregate::Template;
use crate::template::views::TemplateView;
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

#[async_trait]
pub trait CatalogueServices: Send + Sync {
    async fn template_exists(&self, id: &str) -> bool;
}

pub struct CatalogueServiceImpl {
    pub template_view_repo: Arc<dyn ViewRepository<TemplateView, Template>>,
}

#[async_trait]
impl CatalogueServices for CatalogueServiceImpl {
    async fn template_exists(&self, id: &str) -> bool {
        self.template_view_repo.load(id).await.ok().flatten().is_some()
    }
}
