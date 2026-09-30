use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const RATE: u32 = 16_000;
pub const HEADER: u64 = 44;

pub fn header(data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36u32.saturating_add(data_bytes)).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(RATE * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

pub fn write(path: &Path, samples: &[i16]) -> Result<()> {
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(&header((samples.len() * 2) as u32));
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).with_context(|| format!("could not write {}", path.display()))
}

pub fn read(path: &Path) -> Result<Vec<i16>> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .with_context(|| format!("could not read {}", path.display()))?;
    let body = bytes.get(HEADER as usize..).unwrap_or(&[]);
    Ok(body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect())
}

pub fn peak(samples: &[i16]) -> f64 {
    samples
        .iter()
        .map(|s| (*s as i32).unsigned_abs())
        .max()
        .unwrap_or(0) as f64
        / 32768.0
}

pub fn seal(path: &Path) -> Result<()> {
    let len = std::fs::metadata(path)?.len();
    let data = len.saturating_sub(HEADER) & !1;
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.set_len(HEADER + data)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&header(data.min(u32::MAX as u64) as u32))?;
    file.sync_all()?;
    Ok(())
}

pub struct Writer {
    dir: PathBuf,
    segment_samples: u64,
    overlap_samples: usize,
    index: u32,
    file: File,
    written: u64,
    fresh: u64,
    since_header: u64,
    recent: Vec<i16>,
}

pub fn part_name(index: u32) -> String {
    format!("seg-{index:05}.part.wav")
}

pub fn done_name(index: u32) -> String {
    format!("seg-{index:05}.wav")
}

impl Writer {
    pub fn open(
        dir: &Path,
        first_index: u32,
        segment_samples: u64,
        overlap_samples: usize,
    ) -> Result<Writer> {
        let file = Self::create(dir, first_index, &[])?;
        Ok(Writer {
            dir: dir.to_path_buf(),
            segment_samples: segment_samples.max(1),
            overlap_samples,
            index: first_index,
            file,
            written: 0,
            fresh: 0,
            since_header: 0,
            recent: Vec::new(),
        })
    }

    fn create(dir: &Path, index: u32, prefill: &[i16]) -> Result<File> {
        let path = dir.join(part_name(index));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("could not create {}", path.display()))?;
        file.write_all(&header((prefill.len() * 2) as u32))?;
        let bytes: Vec<u8> = prefill.iter().flat_map(|s| s.to_le_bytes()).collect();
        file.write_all(&bytes)?;
        Ok(file)
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn push(&mut self, mut samples: &[i16]) -> Result<Vec<u32>> {
        let mut finished = Vec::new();
        while !samples.is_empty() {
            let room = (self.segment_samples - self.fresh) as usize;
            let take = room.min(samples.len());
            let (now, rest) = samples.split_at(take);
            let bytes: Vec<u8> = now.iter().flat_map(|s| s.to_le_bytes()).collect();
            self.file.write_all(&bytes)?;
            self.written += now.len() as u64;
            self.fresh += now.len() as u64;
            self.since_header += now.len() as u64;
            self.recent.extend_from_slice(now);
            if self.recent.len() > self.overlap_samples * 4 + 1 {
                let keep = self.recent.len() - self.overlap_samples;
                self.recent.drain(..keep);
            }
            if self.since_header >= RATE as u64 {
                self.update_header()?;
            }
            if self.fresh >= self.segment_samples {
                finished.push(self.roll()?);
            }
            samples = rest;
        }
        Ok(finished)
    }

    fn update_header(&mut self) -> Result<()> {
        let prefill = self.file_samples() - self.written;
        let data = ((prefill + self.written) * 2).min(u32::MAX as u64) as u32;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(data))?;
        self.file.seek(SeekFrom::End(0))?;
        self.since_header = 0;
        Ok(())
    }

    fn file_samples(&mut self) -> u64 {
        self.file
            .metadata()
            .map(|m| m.len().saturating_sub(HEADER) / 2)
            .unwrap_or(self.written)
    }

    fn close_current(&mut self) -> Result<u32> {
        self.update_header()?;
        self.file.sync_all()?;
        let index = self.index;
        std::fs::rename(
            self.dir.join(part_name(index)),
            self.dir.join(done_name(index)),
        )?;
        Ok(index)
    }

    fn roll(&mut self) -> Result<u32> {
        let done = self.close_current()?;
        let start = self.recent.len().saturating_sub(self.overlap_samples);
        let prefill: Vec<i16> = self.recent[start..].to_vec();
        self.index += 1;
        self.file = Self::create(&self.dir, self.index, &prefill)?;
        self.written = 0;
        self.fresh = 0;
        self.since_header = 0;
        Ok(done)
    }

    pub fn finish(mut self, least_fresh: u64) -> Result<Option<u32>> {
        if self.fresh >= least_fresh.max(1) {
            return self.close_current().map(Some);
        }
        drop(self.file);
        let _ = std::fs::remove_file(self.dir.join(part_name(self.index)));
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(from: i16, n: usize) -> Vec<i16> {
        (0..n).map(|i| from.wrapping_add(i as i16)).collect()
    }

    #[test]
    fn a_written_wav_reads_back_with_a_correct_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write(&path, &[1, -2, 3]).unwrap();
        assert_eq!(read(&path).unwrap(), vec![1, -2, 3]);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[40..44], &6u32.to_le_bytes());
        assert_eq!(&bytes[24..28], &16_000u32.to_le_bytes());
    }

    #[test]
    fn segments_roll_over_at_their_length_and_carry_the_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Writer::open(dir.path(), 0, 10, 3).unwrap();
        let done = w.push(&ramp(0, 25)).unwrap();
        assert_eq!(done, vec![0, 1]);
        assert_eq!(read(&dir.path().join(done_name(0))).unwrap(), ramp(0, 10));
        assert_eq!(read(&dir.path().join(done_name(1))).unwrap(), ramp(7, 13));
        assert!(dir.path().join(part_name(2)).exists());
        assert_eq!(w.finish(1).unwrap(), Some(2));
        assert_eq!(read(&dir.path().join(done_name(2))).unwrap(), ramp(17, 8));
    }

    #[test]
    fn a_last_part_with_no_new_audio_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Writer::open(dir.path(), 0, 10, 3).unwrap();
        w.push(&ramp(0, 10)).unwrap();
        assert_eq!(w.finish(1).unwrap(), None);
        assert!(!dir.path().join(part_name(1)).exists());
        assert!(dir.path().join(done_name(0)).exists());
    }

    #[test]
    fn a_part_left_by_a_crash_is_readable_after_sealing() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Writer::open(dir.path(), 0, 1_000_000, 0).unwrap();
        w.push(&ramp(0, 20_000)).unwrap();
        w.push(&ramp(5, 101)).unwrap();
        std::mem::forget(w);
        let part = dir.path().join(part_name(0));
        seal(&part).unwrap();
        assert_eq!(read(&part).unwrap().len(), 20_101);
        let bytes = std::fs::read(&part).unwrap();
        assert_eq!(&bytes[40..44], &(20_101u32 * 2).to_le_bytes());
    }

    #[test]
    fn peak_is_a_fraction_of_full_scale() {
        assert_eq!(peak(&[]), 0.0);
        assert_eq!(peak(&[0, -16384, 100]), 0.5);
        assert_eq!(peak(&[i16::MIN]), 1.0);
    }
}
