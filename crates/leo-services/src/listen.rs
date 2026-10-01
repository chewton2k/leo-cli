pub fn peak_amplitude(path: &std::path::Path) -> Option<f64> {
    crate::session::wav::read(path)
        .ok()
        .map(|samples| crate::session::wav::peak(&samples))
}

pub fn microphone_peak(seconds: f64) -> Option<f64> {
    crate::session::mic::listen_for(seconds)
        .ok()
        .map(|samples| crate::session::wav::peak(&samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_amplitude_reads_silence_and_sound_apart() {
        let dir = tempfile::tempdir().unwrap();
        let silent = dir.path().join("silent.wav");
        crate::session::wav::write(&silent, &vec![0i16; 16_000]).unwrap();
        let level = peak_amplitude(&silent).expect("a level for a silent file");
        assert!(
            crate::ai::live::is_silent(level),
            "silence measured {level}"
        );

        let tone = dir.path().join("tone.wav");
        let samples: Vec<i16> = (0..16_000)
            .map(|i| ((i as f64 * 0.17).sin() * 12_000.0) as i16)
            .collect();
        crate::session::wav::write(&tone, &samples).unwrap();
        let level = peak_amplitude(&tone).expect("a level for a tone");
        assert!(
            !crate::ai::live::is_silent(level),
            "a tone measured {level}"
        );
    }

    #[test]
    fn a_missing_file_has_no_level_rather_than_a_wrong_one() {
        assert!(peak_amplitude(std::path::Path::new("/nonexistent/nope.wav")).is_none());
    }
}
