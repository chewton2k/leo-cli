use std::path::Path;

use anyhow::Result;

use crate::ai::chain::{run_transcribe_chain, ChainOutcome};
use crate::ai::provider::build_transcribe_chain;
use crate::config::secret::SecretStore;
use crate::config::Config;

/// Transcribe through the configured chain.
pub fn run(
    cfg: &Config,
    store: &dyn SecretStore,
    audio_path: &Path,
) -> Result<ChainOutcome<String>> {
    let providers = build_transcribe_chain(cfg, store);
    run_transcribe_chain(providers, audio_path, |p, path| p.transcribe(path))
}
