//! Built-in components. Each one uses the same contract as any external plugin.
pub mod core_comp;
pub mod exec;
pub mod peers;
pub mod supervisor;
pub mod system;
pub mod tokens;

pub use core_comp::Core;
pub use exec::Exec;
pub use peers::Peers;
pub use supervisor::Supervisor;
pub use system::System;
pub use tokens::Tokens;
