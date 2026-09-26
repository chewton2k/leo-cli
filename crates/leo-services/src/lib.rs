//! What leo needs from the outside world: AI providers and their fallback
//! chains, configuration and credentials, audio recording, and the checks that
//! say which of those work on this machine. Built on `leo-core`; knows nothing
//! about how it is displayed.

pub mod ai;
pub mod config;
pub mod doctor;
pub mod health;
pub mod listen;
pub mod providers;
pub mod update;
