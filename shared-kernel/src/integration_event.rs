use crate::event_bus::CloudEvent;
use serde::Serialize;

/// A formal Integration Event in the Published Language (PL).
///
/// Unlike internal CQRS domain events which represent private aggregate state changes,
/// integration events establish a stable contract for external consumers,
/// webhook subscribers, and cross-system streaming.
pub trait IntegrationEvent: Serialize + Send + Sync + 'static {
    /// Reverse-DNS event type, e.g. `com.impierce.unicore.issuance.credential.offered`.
    fn event_type(&self) -> &'static str;

    /// Optional entity or aggregate subject ID.
    fn subject(&self) -> Option<String>;

    /// Converts this integration event into a CNCF [`CloudEvent`] v1.0.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization of `self` into a JSON value fails.
    fn into_cloud_event(
        self,
        source: &str,
        caller_id: Option<String>,
        caller_type: Option<String>,
    ) -> Result<CloudEvent, serde_json::Error>
    where
        Self: Sized,
    {
        let data = serde_json::to_value(&self)?;
        let subject = self.subject();
        let event_type = self.event_type();
        let mut ce = CloudEvent::new(event_type, source)
            .with_data(data)
            .with_caller(caller_id, caller_type);
        if let Some(s) = subject {
            ce = ce.with_subject(s);
        }
        Ok(ce)
    }
}
