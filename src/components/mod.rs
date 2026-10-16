//! Built-in components. Each one uses the same contract as any external plugin.
pub mod core_comp;
#[cfg(feature = "native-processes")]
pub mod exec;
pub mod peers;
#[cfg(feature = "native-processes")]
pub mod supervisor;
#[cfg(feature = "host-telemetry")]
pub mod system;
pub mod tokens;

pub use core_comp::Core;
#[cfg(feature = "native-processes")]
pub use exec::Exec;
pub use peers::Peers;
#[cfg(feature = "native-processes")]
pub use supervisor::Supervisor;
#[cfg(feature = "host-telemetry")]
pub use system::System;
pub use tokens::Tokens;

pub mod identity;
pub use identity::Identity;

#[cfg(feature = "native-processes")]
pub mod packages;
#[cfg(feature = "native-processes")]
pub use packages::Packages;
