pub mod admission;
pub mod closure;
pub mod context;
pub mod errors;
pub mod runtime;

use adapter_quantum::quantum_state_event;

pub use errors::KernelError;
pub use runtime::KernelRuntime;

pub fn run_from_env() -> Result<(), KernelError> {
    let profile_path = std::env::var("TNSR_PROFILE")
        .unwrap_or_else(|_| "profiles/default.profile.json".to_string());

    let mut runtime = KernelRuntime::from_profile_path(profile_path)?;
    runtime.register_plugins()?;
    runtime.register_schemas()?;

    let mut event = quantum_state_event(runtime.run_id());
    let admission = runtime.admit_and_record(&mut event)?;

    if !admission.allowed {
        runtime.seal_run_with_reason(station_run::RunStatus::Failed, admission.reason)?;
        return Ok(());
    }

    runtime.publish_admitted(event)?;
    runtime.seal_run(station_run::RunStatus::Completed)?;
    Ok(())
}
