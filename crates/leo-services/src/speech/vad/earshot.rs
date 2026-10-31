use anyhow::Result;

use super::{check_whole_frames, SpeechScorer, VadFrame, VoiceActivityDetector};

pub const EARSHOT_FRAME_SAMPLES: usize = 256;

pub struct EarshotVad {
    engine: Box<earshot::Detector>,
    threshold: f32,
    clamped_frame: [f32; EARSHOT_FRAME_SAMPLES],
}

impl EarshotVad {
    pub fn new(threshold: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&threshold) {
            anyhow::bail!("threshold must be between 0.0 and 1.0");
        }

        Ok(Self {
            engine: earshot::Detector::default_boxed(),
            threshold,
            clamped_frame: [0.0; EARSHOT_FRAME_SAMPLES],
        })
    }
}

impl EarshotVad {
    fn score_frame(&mut self, frame: &[f32]) -> Result<f32> {
        if frame.len() != EARSHOT_FRAME_SAMPLES {
            anyhow::bail!(
                "expected {EARSHOT_FRAME_SAMPLES} samples, got {}",
                frame.len()
            );
        }
        if frame.iter().any(|sample| !sample.is_finite()) {
            anyhow::bail!("Earshot VAD input contained a non-finite sample");
        }

        Ok(
            if frame.iter().all(|sample| (-1.0..=1.0).contains(sample)) {
                self.engine.predict_f32(frame)
            } else {
                for (clamped, sample) in self.clamped_frame.iter_mut().zip(frame) {
                    *clamped = sample.clamp(-1.0, 1.0);
                }
                self.engine.predict_f32(&self.clamped_frame)
            },
        )
    }
}

impl VoiceActivityDetector for EarshotVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        if self.score_frame(frame)? >= self.threshold {
            Ok(VadFrame::Speech(frame))
        } else {
            Ok(VadFrame::Noise)
        }
    }

    fn frame_samples(&self) -> usize {
        EARSHOT_FRAME_SAMPLES
    }

    fn reset(&mut self) {
        self.engine.reset();
    }
}

impl SpeechScorer for EarshotVad {
    fn score(&mut self, frames: &[f32]) -> Result<Vec<f32>> {
        check_whole_frames(frames, EARSHOT_FRAME_SAMPLES)?;
        let (frames, _) = frames.as_chunks::<EARSHOT_FRAME_SAMPLES>();
        frames.iter().map(|frame| self.score_frame(frame)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_frame_size() {
        let mut vad = EarshotVad::new(0.5).unwrap();
        assert!(vad.push_frame(&[0.0; EARSHOT_FRAME_SAMPLES - 1]).is_err());
    }

    #[test]
    fn silence_is_not_voice() {
        let mut vad = EarshotVad::new(0.5).unwrap();
        assert!(!vad
            .push_frame(&[0.0; EARSHOT_FRAME_SAMPLES])
            .unwrap()
            .is_speech());
    }

    #[test]
    fn validates_threshold() {
        assert!(EarshotVad::new(-0.1).is_err());
        assert!(EarshotVad::new(1.1).is_err());
    }

    #[test]
    fn clamps_resampler_overshoot_without_changing_output() {
        let mut vad = EarshotVad::new(0.0).unwrap();
        let frame = [1.01; EARSHOT_FRAME_SAMPLES];
        match vad.push_frame(&frame).unwrap() {
            VadFrame::Speech(output) => assert_eq!(output, frame),
            VadFrame::Noise => panic!("zero threshold should retain the frame"),
        }
    }

    #[test]
    fn rejects_non_finite_audio() {
        let mut vad = EarshotVad::new(0.5).unwrap();
        let mut frame = [0.0; EARSHOT_FRAME_SAMPLES];
        frame[0] = f32::NAN;
        assert!(vad.push_frame(&frame).is_err());
    }
}
