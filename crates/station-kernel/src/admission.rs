use runtime_core::EventEnvelope;
use station_policy::{EventAdmission, PolicyEngine};

use crate::{context::KernelContext, errors::KernelError};

pub fn admit_and_record(
    context: &mut KernelContext,
    event: &mut EventEnvelope,
) -> Result<EventAdmission, KernelError> {
    let policy = PolicyEngine {
        plugins: &context.registry,
        schemas: &context.schemas,
        supervisor: &context.supervisor,
    };

    let admission = policy.admit_event(event)?;

    if let Some(policy_event) = &admission.policy_event {
        context.replay.append_record(policy_event)?;
    }

    Ok(admission)
}
