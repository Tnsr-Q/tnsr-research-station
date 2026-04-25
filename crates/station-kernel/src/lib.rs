mod admission;
mod closure;
mod context;
pub mod errors;
mod runtime;

use adapter_quantum::quantum_state_event;

pub use admission::AdmittedEvent;
pub use errors::KernelError;
pub use runtime::KernelRuntime;

pub fn run_from_env() -> Result<(), KernelError> {
    let profile_path = std::env::var("TNSR_PROFILE")
        .unwrap_or_else(|_| "profiles/default.profile.json".to_string());

    let mut runtime = KernelRuntime::from_profile_path(profile_path)?;
    runtime.register_plugins()?;
    runtime.register_schemas()?;

    let event = quantum_state_event(runtime.run_id());
    let admitted = match runtime.admit(event)? {
        Ok(admitted) => admitted,
        Err(denial) => {
            runtime.seal_run_with_reason(station_run::RunStatus::Failed, denial.reason)?;
            return Ok(());
        }
    };

    runtime.publish_admitted(admitted)?;
    runtime.seal_run(station_run::RunStatus::Completed)?;
    Ok(())
}
