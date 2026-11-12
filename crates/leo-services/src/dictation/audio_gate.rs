use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
#[derive(Default)]
pub struct AudioGate {
    recordings: AtomicUsize,
    sounds: AtomicUsize,
}
pub struct Lease<'a>(&'a AtomicUsize);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub fn gate() -> &'static AudioGate {
    static GATE: std::sync::OnceLock<AudioGate> = std::sync::OnceLock::new();
    GATE.get_or_init(AudioGate::default)
}
impl AudioGate {
    pub fn sound(&self) -> Option<Lease<'_>> {
        if self.recordings.load(Ordering::SeqCst) > 0 {
            return None;
        }
        self.sounds.fetch_add(1, Ordering::SeqCst);
        let lease = Lease(&self.sounds);
        if self.recordings.load(Ordering::SeqCst) > 0 {
            return None;
        }
        Some(lease)
    }
    pub fn recording(&self) -> anyhow::Result<Lease<'_>> {
        self.recordings.fetch_add(1, Ordering::SeqCst);
        let lease = Lease(&self.recordings);
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.sounds.load(Ordering::SeqCst) > 0 {
            anyhow::ensure!(
                Instant::now() < deadline,
                "A feedback sound did not finish. Try starting the recording again."
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(lease)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    #[test]
    fn recording_waits_for_an_existing_sound_and_blocks_new_sounds() {
        let gate = Arc::new(super::AudioGate::default());
        let sound = gate.sound().unwrap();
        let worker = gate.clone();
        let (done, ready) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let recording = worker.recording().unwrap();
            done.send(()).unwrap();
            released.recv().unwrap();
            drop(recording);
        });
        assert!(ready.recv_timeout(Duration::from_millis(30)).is_err());
        let now = Instant::now();
        drop(sound);
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(now.elapsed() < Duration::from_secs(1));
        assert!(gate.sound().is_none());
        release.send(()).unwrap();
        thread.join().unwrap();
        assert!(gate.sound().is_some());
    }
}
