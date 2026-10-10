//! The domain: notes, the store that keeps them on disk, and the one command
//! vocabulary every front end speaks. Nothing here draws to a terminal or
//! makes a network request, which is what keeps it testable on its own.

pub mod action;
pub mod attachments;
pub mod diag;
pub mod editor;
pub mod filename;
pub mod keep;
pub mod manual;
pub mod notes;
pub mod open;
pub mod paths;
pub mod recording;
pub mod store;
pub mod sync;
