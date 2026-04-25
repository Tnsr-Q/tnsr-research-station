use runtime_core::EventEnvelope;
use station_policy::{EventAdmission, PolicyEngine};

use crate::{context::KernelContext, errors::KernelError};

#[derive(Debug, Clone)]
pub struct AdmittedEvent {
    event: EventEnvelope,
    policy_event_id: String,
}

impl AdmittedEvent {
    pub(crate) fn new(event: EventEnvelope, policy_event_id: String) -> Self {
        Self {
            event,
            policy_event_id,
        }
    }

    pub(crate) fn into_parts(self) -> (EventEnvelope, String) {
        (self.event, self.policy_event_id)
    }

    pub fn policy_event_id(&self) -> &str {
        &self.policy_event_id
    }

    pub fn event_id(&self) -> &str {
        &self.event.event_id
    }

    pub fn topic(&self) -> &str {
        &self.event.topic
    }

    pub fn source(&self) -> &str {
        &self.event.source
    }
}

pub fn admit_and_record(
    context: &mut KernelContext,
    mut event: EventEnvelope,
) -> Result<Result<AdmittedEvent, EventAdmission>, KernelError> {
    let policy = PolicyEngine {
        plugins: &context.registry,
        schemas: &context.schemas,
        supervisor: &context.supervisor,
    };

    let admission = policy.admit_event(&mut event)?;

    if let Some(policy_event) = &admission.policy_event {
        context.replay.append_record(policy_event)?;
    }

    if admission.allowed {
        let policy_event_id = admission
            .policy_event
            .as_ref()
            .map(|event| event.event_id.clone())
            .unwrap_or_default();
        return Ok(Ok(AdmittedEvent::new(event, policy_event_id)));
    }

    Ok(Err(admission))
}
