use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    EventEnvelope, Query, View,
};
use oid4vci::{credential_response::CredentialResponse, token_response::TokenResponse};
use serde::{Deserialize, Serialize};

use crate::offer::aggregate::Offer;

pub struct OfferSubQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    pub view_repository: Arc<R>,
    pub r#type: String,
    pub _phantom: PhantomData<V>,
}

impl<R, V> OfferSubQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    pub fn new(view_repository: Arc<R>, r#type: String) -> Self {
        OfferSubQuery {
            view_repository,
            r#type,
            _phantom: PhantomData,
        }
    }

    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError> {
        match self.view_repository.load_with_context(&view_id).await? {
            None => {
                let view_context = ViewContext::new(view_id, 0);
                Ok((Default::default(), view_context))
            }
            Some((view, context)) => Ok((view, context)),
        }
    }

    pub(crate) async fn apply_events(
        &self,
        view_id: &str,
        events: &[EventEnvelope<Offer>],
    ) -> Result<(), PersistenceError> {
        let (mut view, mut view_context) = self.load_mut(view_id.to_string()).await?;
        println!("Applying events to view: {:?}", view);
        println!("view_context: {:?}", view_context.view_instance_id);

        for event in events {
            match &event.payload {
                crate::offer::event::OfferEvent::OfferCreated {
                    pre_authorized_code,
                    access_token,
                } => {
                    println!("own type: {:?}", self.r#type);
                    if self.r#type == "pre-authorized_code" {
                        println!("pre_authorized_code: {:?}", pre_authorized_code);
                        view_context.view_instance_id = pre_authorized_code.clone();
                    } else if self.r#type == "access_token" {
                        println!("access_token: {:?}", access_token);
                        view_context.view_instance_id = access_token.clone();
                    }
                }
                _ => {}
            }
            println!("event: {:?}", event.payload);
            view.update(event);
        }
        println!("view_context: {:?}", view_context.view_instance_id);
        self.view_repository.update_view(view, view_context).await?;
        Ok(())
    }
}

#[async_trait]
impl<R, V> Query<Offer> for OfferSubQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    async fn dispatch(&self, view_id: &str, events: &[EventEnvelope<Offer>]) {
        match self.apply_events(view_id, events).await {
            Ok(_) => {}
            Err(err) => {}
        };
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct OfferView {
    pub credential_id: String,
    pub pre_authorized_code: String,
    pub access_token: String,
    pub form_urlencoded_credential_offer: String,
    pub token_response: Option<TokenResponse>,
    pub credential_response: Option<CredentialResponse>,
}

impl View<Offer> for OfferView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            OfferCreated {
                pre_authorized_code,
                access_token,
            } => {
                self.pre_authorized_code = pre_authorized_code.clone();
                self.access_token = access_token.clone();
            }
            CredentialAdded { credential_id } => {
                self.credential_id = credential_id.clone();
            }
            CredentialOfferCreated {
                form_url_encoded_credential_offer,
            } => {
                self.form_urlencoded_credential_offer = form_url_encoded_credential_offer.clone();
            }
            TokenResponseCreated { token_response } => {
                self.token_response.replace(token_response.clone());
            }
            CredentialResponseCreated { credential_response } => {
                self.credential_response.replace(credential_response.clone());
            }
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PreAuthorizedCodeView {
    pub offer_id: String,
}

impl View<Offer> for PreAuthorizedCodeView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            OfferCreated { .. } => {
                self.offer_id = event.aggregate_id.clone();
            }
            _ => {}
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AccessTokenView {
    pub offer_id: String,
}

impl View<Offer> for AccessTokenView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            OfferCreated { .. } => {
                self.offer_id = event.aggregate_id.clone();
            }
            _ => {}
        }
    }
}
