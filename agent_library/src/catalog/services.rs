use crate::template::aggregate::Template;
use crate::template::views::TemplateView;
use async_trait::async_trait;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;

#[async_trait]
pub trait CatalogServices: Send + Sync {
    async fn check_all_templates_exist(&self, ids: &[String]) -> Vec<String>;
}

pub struct CatalogServiceImpl {
    pub template_view_repo: Arc<dyn DynViewRepository<TemplateView, Template>>,
}

#[async_trait]
impl CatalogServices for CatalogServiceImpl {
    async fn check_all_templates_exist(&self, ids: &[String]) -> Vec<String> {
        let mut templates = Vec::new();
        for id in ids {
            if self.template_view_repo.load(id).await.ok().flatten().is_none() {
                templates.push(id.clone());
            }
        }
        templates
    }
}
