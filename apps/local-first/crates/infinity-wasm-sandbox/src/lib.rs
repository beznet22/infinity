//! Untrusted Wasm tools, with nothing to reach the host through.
//!
//! The browser does not need this crate: there the engine *is* the sandbox, and a
//! Worker plus an import allow-list play the same role (D29). On Android the
//! engine has to be embedded, so this is where the rules live.
//!
//! Three guarantees, each enforced by a test:
//! 1. **No host imports.** A module that imports anything is refused before it
//!    runs, so there is no path to the filesystem, the network or the clock.
//! 2. **Bounded memory.** A `StoreLimits` cap stops a module from growing memory
//!    without end.
//! 3. **Bounded time.** Fuel metering stops a tool that never returns.

use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

/// How much linear memory a single tool may claim (1 MiB).
pub const MEMORY_LIMIT_BYTES: usize = 1 << 20;
/// How much work a single run may do before fuel runs out.
pub const FUEL_LIMIT: u64 = 10_000_000;

/// Why a tool was refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// The bytes are not a valid Wasm module.
    Compile(String),
    /// The module wants a host function, which this sandbox never provides.
    Imports { module: String, name: String },
    /// Instantiation or the call itself failed (including fuel exhaustion).
    Trap(String),
    /// The module has no exported `run` function returning an `i32`.
    NoEntryPoint,
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compile(error) => write!(formatter, "compile: {error}"),
            Self::Imports { module, name } => {
                write!(
                    formatter,
                    "denied: imports {module}.{name}, the sandbox has none"
                )
            }
            Self::Trap(error) => write!(formatter, "trap: {error}"),
            Self::NoEntryPoint => write!(formatter, "no exported `run` function returning i32"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// Host state; only the memory limiter lives here.
#[derive(Default)]
struct HostState {
    limits: StoreLimits,
}

/// Run a module's `run` export to completion under the sandbox rules.
///
/// Returns the export's `i32` value. Any import is refused, memory is capped and
/// fuel bounds the run time.
pub fn run(bytes: &[u8]) -> Result<i32, SandboxError> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module =
        Module::new(&engine, bytes).map_err(|error| SandboxError::Compile(error.to_string()))?;

    if let Some(import) = module.imports().next() {
        return Err(SandboxError::Imports {
            module: import.module().to_owned(),
            name: import.name().to_owned(),
        });
    }

    let mut store = Store::new(
        &engine,
        HostState {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT_BYTES)
                .build(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store
        .set_fuel(FUEL_LIMIT)
        .map_err(|error| SandboxError::Trap(error.to_string()))?;

    let linker = Linker::new(&engine);
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|error| SandboxError::Trap(error.to_string()))?;
    let entry = instance
        .get_typed_func::<(), i32>(&store, "run")
        .map_err(|_| SandboxError::NoEntryPoint)?;
    entry
        .call(&mut store, ())
        .map_err(|error| SandboxError::Trap(error.to_string()))
}

/// The modules below are embedded so the device can prove the rules every launch.
const BENIGN: &[u8] = include_bytes!("../assets/echo42.wasm");
const HOSTILE: &[u8] = include_bytes!("../assets/wasi_write.wasm");
const SPIN: &[u8] = include_bytes!("../assets/spin.wasm");

/// Run the three reference tools and report each outcome.
///
/// The probe runs this on every launch: if a future change weakens an import
/// check or removes the fuel budget, the report says so on a real device.
pub fn self_test() -> [(&'static str, Result<i32, String>); 3] {
    [
        ("benign", run(BENIGN).map_err(|error| error.to_string())),
        ("hostile", run(HOSTILE).map_err(|error| error.to_string())),
        ("spin", run(SPIN).map_err(|error| error.to_string())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_tool_runs_and_returns_its_value() {
        assert_eq!(run(BENIGN).expect("benign tool"), 42);
    }

    #[test]
    fn a_tool_that_wants_host_access_is_refused_before_it_runs() {
        let error = run(HOSTILE).expect_err("must not run");
        assert_eq!(
            error,
            SandboxError::Imports {
                module: "wasi_snapshot_preview1".to_owned(),
                name: "fd_write".to_owned(),
            }
        );
    }

    #[test]
    fn a_tool_that_never_returns_runs_out_of_fuel() {
        let error = run(SPIN).expect_err("must not run forever");
        assert!(matches!(error, SandboxError::Trap(_)), "got {error:?}");
    }

    #[test]
    fn garbage_bytes_are_rejected_not_executed() {
        assert!(matches!(run(b"not wasm"), Err(SandboxError::Compile(_))));
    }
}
