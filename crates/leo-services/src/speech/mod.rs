pub mod catalog;
pub mod library;

pub mod chunking;
pub mod engine;
mod protocol;
pub mod supervisor;
pub mod vad;
mod worker;

const WORKER_FLAG: &str = "--leo-speech-worker";
const CPU_ONLY_FLAG: &str = "--cpu-only";
const LOG_LEVEL_ENV: &str = "LEO_TRANSCRIBE_WORKER_LOG";

mod constants {
    pub const WHISPER_SAMPLE_RATE: u32 = 16_000;
}

pub fn is_worker_invocation() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == WORKER_FLAG)
}
pub fn run_worker() -> i32 {
    worker::run()
}

pub fn prepare(pcm: Vec<f32>) -> anyhow::Result<Vec<Vec<f32>>> {
    anyhow::ensure!(
        pcm.len() <= 320 * 16_000 && pcm.iter().all(|s| s.is_finite()),
        "This audio is too long or damaged. Try a shorter recording."
    );
    let mut chunker = chunking::Chunker::new(Box::new(|| Ok(Box::new(vad::EarshotVad::new(0.5)?))));
    let mut chunks = chunker.push(pcm);
    chunks.extend(chunker.finish());
    Ok(chunks)
}

#[cfg(test)]
mod tests;
