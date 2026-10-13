pub mod all_offers;

use crate::offer::aggregate::{DeliveryOptions, Offer};
use cqrs_es::{EventEnvelope, View};

pub type OfferView = Offer;

impl View<Offer> for Offer {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            CredentialOfferCreated {
                offer_id,
                grant_types,
                status,
                credential_offer,
                credential_offer_uri,
                pre_authorized_code,
                tx_code,
                delivery_options,
            } => {
                self.offer_id.clone_from(offer_id);
                self.grant_types.clone_from(grant_types);
                self.status.clone_from(status);
                self.credential_offer.replace(credential_offer.clone());
                self.credential_offer_uri.replace(credential_offer_uri.clone());
                self.pre_authorized_code.clone_from(pre_authorized_code);
                self.tx_code.clone_from(tx_code);
                self.delivery_options.clone_from(delivery_options);
            }
            CredentialsAdded {
                offer_id,
                credential_ids,
                credential_offer,
            } => {
                self.offer_id.clone_from(offer_id);
                // The event carries only the IDs added by this command, so extend rather than replace.
                for credential_id in credential_ids {
                    if !self.credential_ids.contains(credential_id) {
                        self.credential_ids.push(credential_id.clone());
                    }
                }
                self.credential_offer.replace(credential_offer.clone());
            }
            FormUrlEncodedCredentialOfferCreated {
                offer_id,
                form_url_encoded_credential_offer,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.form_url_encoded_credential_offer
                    .replace(form_url_encoded_credential_offer.clone());
                self.status.clone_from(status);
            }
            // The latest delivery replaces any earlier one, so the view tells who the offer last went to.
            CredentialOfferSent {
                offer_id,
                target_url,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.status.clone_from(status);
                self.delivery_options = Some(DeliveryOptions {
                    recipient_email: None,
                    target_url: Some(target_url.clone()),
                });
            }
            CredentialOfferEmailSent {
                offer_id,
                recipient_email,
                form_url_encoded_credential_offer: _form_url_encoded_credential_offer,
                offer_link: _offer_link,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.status.clone_from(status);
                self.delivery_options = Some(DeliveryOptions {
                    recipient_email: Some(recipient_email.clone()),
                    target_url: None,
                });
            }
            CredentialRequestVerified { offer_id, subject_id } => {
                self.offer_id.clone_from(offer_id);
                self.subject_id.clone_from(subject_id);
            }
            CredentialResponseCreated {
                offer_id,
                credential_response,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.credential_response.replace(credential_response.clone());
                self.status.clone_from(status);
                self.successful_issuances = self.successful_issuances.saturating_add(1);
            }
            TxCodeGenerated {
                offer_id,
                tx_code,
                delivery_options,
            } => {
                self.offer_id.clone_from(offer_id);
                self.tx_code.replace(tx_code.clone());
                self.delivery_options.clone_from(delivery_options);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::{aggregate::Status, event::OfferEvent};
    use url::Url;

    fn envelope(sequence: usize, payload: OfferEvent) -> EventEnvelope<Offer> {
        EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence,
            payload,
            metadata: Default::default(),
        }
    }

    fn email_sent(sequence: usize, recipient_email: &str) -> EventEnvelope<Offer> {
        envelope(
            sequence,
            OfferEvent::CredentialOfferEmailSent {
                offer_id: "offer-1".to_string(),
                recipient_email: recipient_email.to_string(),
                form_url_encoded_credential_offer: "openid-credential-offer://".to_string(),
                offer_link: Url::parse("https://issuer.example/public/offers/offer-1").unwrap(),
                status: Status::Pending,
            },
        )
    }

    fn sent_to_organization(sequence: usize, target_url: &str) -> EventEnvelope<Offer> {
        envelope(
            sequence,
            OfferEvent::CredentialOfferSent {
                offer_id: "offer-1".to_string(),
                target_url: Url::parse(target_url).unwrap(),
                status: Status::Pending,
            },
        )
    }

    #[test]
    fn email_delivery_records_the_recipient_email() {
        let mut view = OfferView::default();

        view.update(&email_sent(1, "holder@example.test"));

        assert_eq!(
            view.delivery_options,
            Some(DeliveryOptions {
                recipient_email: Some("holder@example.test".to_string()),
                target_url: None,
            })
        );
    }

    #[test]
    fn organization_delivery_records_the_target_url() {
        let mut view = OfferView::default();

        view.update(&sent_to_organization(1, "https://org.example/offers"));

        assert_eq!(
            view.delivery_options,
            Some(DeliveryOptions {
                recipient_email: None,
                target_url: Some(Url::parse("https://org.example/offers").unwrap()),
            })
        );
    }

    #[test]
    fn latest_delivery_replaces_earlier_ones() {
        let mut view = OfferView::default();

        view.update(&email_sent(1, "first@example.test"));
        view.update(&sent_to_organization(2, "https://org.example/offers"));
        view.update(&email_sent(3, "second@example.test"));

        assert_eq!(
            view.delivery_options,
            Some(DeliveryOptions {
                recipient_email: Some("second@example.test".to_string()),
                target_url: None,
            })
        );
    }
}
