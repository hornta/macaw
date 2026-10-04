//! Platform-independent core of Macaw: the key model, configuration, mapping tables and the
//! remapping engine. Nothing in this crate talks to Windows, so all of it is unit-testable.

pub mod config;
pub mod engine;
pub mod key;
pub mod profile;
pub mod sim;
pub mod tables;

pub use config::Config;
pub use engine::{Action, Ctx, Decision, Engine, Input, Out, Typematic};
pub use key::{Key, WinMods};
pub use profile::AppProfile;
