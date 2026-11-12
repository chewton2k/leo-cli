use anyhow::Result;

use super::constants;

pub const VAD_PREFILL_MS: u64 = 450;
pub const VAD_OFFLINE_HANGOVER_MS: u64 = 450;
pub const VAD_STREAMING_HANGOVER_MS: u64 = 1650;
pub const VAD_ONSET_MS: u64 = 60;

pub const fn frames_for_duration_ms(duration_ms: u64, frame_samples: usize) -> usize {
    assert!(frame_samples > 0, "VAD frame size must be non-zero");
    let numerator = duration_ms * constants::WHISPER_SAMPLE_RATE as u64;
    let denominator = frame_samples as u64 * 1000;
    numerator.div_ceil(denominator) as usize
}

pub enum VadFrame<'a> {
    Speech(&'a [f32]),
    Noise,
}

impl<'a> VadFrame<'a> {
    #[inline]
    pub fn is_speech(&self) -> bool {
        matches!(self, VadFrame::Speech(_))
    }
}

pub trait VoiceActivityDetector: Send + Sync {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>>;

    fn frame_samples(&self) -> usize;

    fn is_voice(&mut self, frame: &[f32]) -> Result<bool> {
        Ok(self.push_frame(frame)?.is_speech())
    }

    fn reset(&mut self) {}
}

pub trait SpeechScorer: VoiceActivityDetector {
    fn score(&mut self, frames: &[f32]) -> Result<Vec<f32>>;
}

fn check_whole_frames(frames: &[f32], frame_samples: usize) -> Result<()> {
    if !frames.len().is_multiple_of(frame_samples) {
        anyhow::bail!(
            "expected whole {frame_samples}-sample frames, got {} samples",
            frames.len()
        );
    }
    Ok(())
}

mod earshot;
mod silero;
mod smoothed;

pub use earshot::EarshotVad;
pub use silero::{dir, SileroVad, SILERO_VAD_THRESHOLD};
pub use smoothed::SmoothedVad;

pub fn detector() -> Result<Box<dyn SpeechScorer>> {
    match SileroVad::new(SILERO_VAD_THRESHOLD) {
        Ok(silero) => Ok(Box::new(silero)),
        Err(error) => {
            log::warn!("Silero VAD is unavailable, using Earshot: {error:#}");
            Ok(Box::new(EarshotVad::new(0.5)?))
        }
    }
}

pub struct CaptureVad {
    vad: SmoothedVad,
    pending: Vec<f32>,
    frame: usize,
}

impl CaptureVad {
    pub fn new(detector: Box<dyn VoiceActivityDetector>, hangover_ms: u64) -> Self {
        let frame = detector.frame_samples();
        Self {
            vad: SmoothedVad::new(
                detector,
                frames_for_duration_ms(VAD_PREFILL_MS, frame),
                frames_for_duration_ms(hangover_ms, frame),
                frames_for_duration_ms(VAD_ONSET_MS, frame),
            ),
            pending: Vec::new(),
            frame,
        }
    }

    fn keep(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        match self.vad.push_frame(frame) {
            Ok(VadFrame::Speech(heard)) => out.extend_from_slice(heard),
            Ok(VadFrame::Noise) => {}
            Err(_) => out.extend_from_slice(frame),
        }
    }

    pub fn push(&mut self, chunk: &[f32]) -> Vec<f32> {
        let mut audio = std::mem::take(&mut self.pending);
        audio.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut frames = audio.chunks_exact(self.frame);
        for frame in &mut frames {
            self.keep(frame, &mut out);
        }
        self.pending = frames.remainder().to_vec();
        out
    }

    pub fn finish(mut self) -> Vec<f32> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let mut frame = std::mem::take(&mut self.pending);
            frame.resize(self.frame, 0.0);
            self.keep(&frame, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scripted(Vec<bool>);
    impl VoiceActivityDetector for Scripted {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
            if self.0.remove(0) {
                Ok(VadFrame::Speech(frame))
            } else {
                Ok(VadFrame::Noise)
            }
        }
        fn frame_samples(&self) -> usize {
            256
        }
    }
    struct Broken;
    impl VoiceActivityDetector for Broken {
        fn push_frame<'a>(&'a mut self, _: &'a [f32]) -> Result<VadFrame<'a>> {
            anyhow::bail!("broken")
        }
        fn frame_samples(&self) -> usize {
            256
        }
    }

    #[test]
    fn capture_keeps_pre_roll_and_a_voiced_final_partial_frame() {
        let mut script = vec![false; 2];
        script.extend(vec![true; 6]);
        let mut vad = CaptureVad::new(Box::new(Scripted(script)), VAD_OFFLINE_HANGOVER_MS);
        let mut kept = vad.push(&[0.1; 256 * 7]);
        assert_eq!(kept.len(), 256 * 7);
        vad.push(&[0.2; 100]);
        kept.extend(vad.finish());
        assert_eq!(kept.len(), 256 * 8);
        assert_eq!(kept[256 * 7 + 99], 0.2);
        assert_eq!(kept[256 * 7 + 100], 0.0);
    }

    struct Flaky(Vec<Option<bool>>);
    impl VoiceActivityDetector for Flaky {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
            match self.0.remove(0) {
                Some(true) => Ok(VadFrame::Speech(frame)),
                Some(false) => Ok(VadFrame::Noise),
                None => anyhow::bail!("flaky"),
            }
        }
        fn frame_samples(&self) -> usize {
            512
        }
    }

    #[test]
    fn a_frame_the_detector_failed_on_is_kept_once() {
        let script = vec![Some(false), None, Some(false), Some(true), Some(true)];
        let mut vad = CaptureVad::new(Box::new(Flaky(script)), VAD_OFFLINE_HANGOVER_MS);
        let audio: Vec<f32> = (0..5).flat_map(|k| vec![k as f32; 512]).collect();
        let kept: Vec<f32> = vad.push(&audio).chunks(512).map(|f| f[0]).collect();
        assert_eq!(kept, [1.0, 0.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn a_failing_detector_keeps_the_audio() {
        let mut vad = CaptureVad::new(Box::new(Broken), VAD_OFFLINE_HANGOVER_MS);
        assert_eq!(vad.push(&[0.1; 256]).len(), 256);
    }

    #[test]
    fn silero_ignores_silence_and_keeps_the_pinned_model() {
        let digest = <sha2::Sha256 as sha2::Digest>::digest(silero::MODEL);
        assert_eq!(
            crate::providers::hex(&digest),
            "0d871a800c420fdd5d1b17a3aea4296d1c39608f0ff4a9295eb76457eb1c88e6"
        );
        let mut vad = SileroVad::new(SILERO_VAD_THRESHOLD).unwrap();
        let frame = vec![0.0; vad.frame_samples()];
        assert!(!vad.push_frame(&frame).unwrap().is_speech());
    }

    #[test]
    fn duration_profiles_preserve_silero_timings() {
        assert_eq!(frames_for_duration_ms(VAD_PREFILL_MS, 512), 15);
        assert_eq!(frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, 512), 15);
        assert_eq!(frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, 512), 52);
        assert_eq!(frames_for_duration_ms(VAD_ONSET_MS, 512), 2);
    }

    #[test]
    fn duration_profiles_round_up_for_earshot_frames() {
        assert_eq!(frames_for_duration_ms(VAD_PREFILL_MS, 256), 29);
        assert_eq!(frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, 256), 29);
        assert_eq!(frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, 256), 104);
        assert_eq!(frames_for_duration_ms(VAD_ONSET_MS, 256), 4);
    }
}
