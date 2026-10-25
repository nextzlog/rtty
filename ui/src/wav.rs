use std::path::Path;
use std::time::{Duration, SystemTime};

pub fn read(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let all: Result<Vec<f32>, _> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect()
        }
    };
    let all = all.map_err(|e| e.to_string())?;
    let ch = spec.channels.max(1) as usize;
    let mono = all
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

pub fn write(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for v in samples {
        let v = (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        w.write_sample(v).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

pub(crate) struct Playback {
    samples: Vec<f32>,
    rate: u32,
    pos: usize,
    start: Option<(SystemTime, SystemTime)>,
}

impl Playback {
    pub fn new(mut samples: Vec<f32>, rate: u32) -> Self {
        let rate = rate.max(1);
        samples.extend(vec![0.0; rate as usize]);
        Playback {
            samples,
            rate,
            pos: 0,
            start: None,
        }
    }

    pub fn take(
        &mut self,
        now: SystemTime,
        align: impl FnOnce(SystemTime) -> SystemTime,
    ) -> Option<(u32, Vec<f32>, SystemTime)> {
        let (t0, at) = *self.start.get_or_insert_with(|| (now, align(now)));
        let secs = now.duration_since(t0).unwrap_or_default().as_secs_f64();
        let due = ((secs * self.rate as f64) as usize).min(self.samples.len());
        if due <= self.pos {
            return None;
        }
        let at = at + Duration::from_secs_f64(self.pos as f64 / self.rate as f64);
        let x = self.samples[self.pos..due].to_vec();
        self.pos = due;
        Some((self.rate, x, at))
    }

    pub fn done(&self) -> bool {
        self.pos >= self.samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_file_reads_back_and_plays_in_real_time() {
        let dir = std::env::temp_dir().join(format!("rtty-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        let x: Vec<f32> = (0..8000).map(|n| (n as f32 * 0.01).sin() * 0.5).collect();
        write(&path, &x, 8000).unwrap();
        let (y, rate) = read(&path).unwrap();
        assert_eq!((y.len(), rate), (8000, 8000));
        assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-3));
        std::fs::remove_dir_all(&dir).unwrap();
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut p = Playback::new(y, rate);
        assert!(p.take(t, |t| t - Duration::from_secs(1)).is_none());
        let (_, a, at) = p.take(t + Duration::from_millis(500), |t| t).unwrap();
        assert_eq!((a.len(), at), (4000, t - Duration::from_secs(1)));
        let (_, b, at) = p.take(t + Duration::from_secs(9), |t| t).unwrap();
        assert_eq!((b.len(), at), (12000, t - Duration::from_millis(500)));
        assert!(p.done());
    }
}
