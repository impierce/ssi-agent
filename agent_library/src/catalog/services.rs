use crate::template::aggregate::Template;
use crate::template::views::TemplateView;
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;

#[async_trait]
pub trait CatalogServices: Send + Sync {
    async fn template_exists(&self, id: &str) -> bool;
    async fn missing_templates(&self, ids: &[String]) -> Vec<String>;
}

pub struct CatalogServiceImpl {
    pub template_view_repo: Arc<dyn ViewRepository<TemplateView, Template>>,
}

#[async_trait]
impl CatalogServices for CatalogServiceImpl {
    async fn template_exists(&self, id: &str) -> bool {
        self.template_view_repo.load(id).await.ok().flatten().is_some()
    }

    async fn missing_templates(&self, ids: &[String]) -> Vec<String> {
        let mut missing = Vec::new();
        for id in ids {
            if !self.template_exists(id).await {
                missing.push(id.clone());
            }
        }
        missing
    }
}
