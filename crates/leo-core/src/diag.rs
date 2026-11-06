//! Where background diagnostics go.
//!
//! Anything below the UI layer — a git auto-commit, a keychain that will not
//! answer, a malformed config, chunk progress during a long transcription —
//! reports through here instead of writing to stderr itself.

/// Report something the user may want to know but that is not a failure of the
/// thing they asked for.
pub fn warn(message: impl Into<String>) {
    eprintln!("  {}", message.into());
}
