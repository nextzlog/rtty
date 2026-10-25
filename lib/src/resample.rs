pub(crate) struct Input {
    rate: u32,
    step: f64,
    fir: Vec<f32>,
    raw: Vec<f32>,
    raw_t0: i128,
    raw_count: u64,
    history: Vec<f32>,
    start: i128,
    pos: f64,
    started: bool,
}

pub(crate) fn lowpass(rate: u32, cutoff: f64) -> Vec<f32> {
    let fc = cutoff / rate as f64;
    if fc >= 0.5 {
        return vec![1.0];
    }
    let len = ((4.0 / (0.5 - fc).min(fc)) as usize).clamp(8, 1024) | 1;
    let mid = (len / 2) as f64;
    let h: Vec<f64> = (0..len)
        .map(|i| {
            let x = i as f64 - mid;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (std::f64::consts::TAU * fc * x).sin() / (std::f64::consts::PI * x)
            };
            let w = std::f64::consts::TAU * i as f64 / (len - 1) as f64;
            sinc * (0.42 - 0.5 * w.cos() + 0.08 * (2.0 * w).cos())
        })
        .collect();
    let sum: f64 = h.iter().sum();
    h.iter().map(|v| (v / sum) as f32).collect()
}

fn cutoff(from: u32, to: u32) -> f64 {
    0.4 * from.min(to) as f64
}

impl Input {
    pub fn new(rate: u32, to: u32) -> Self {
        let fir = if to < rate {
            lowpass(rate, cutoff(rate, to))
        } else {
            vec![1.0]
        };
        Input {
            rate,
            step: rate as f64 / to as f64,
            fir,
            raw: Vec::new(),
            raw_t0: 0,
            raw_count: 0,
            history: Vec::new(),
            start: 0,
            pos: 0.0,
            started: false,
        }
    }

    fn ns(&self) -> f64 {
        1e9 / self.rate as f64
    }

    pub fn convert(&mut self, samples: &[f32], t: i128) -> (Vec<f32>, i128) {
        let ns = self.ns();
        let expected = self.raw_t0 + (self.raw_count as f64 * ns).round() as i128;
        if !self.started || (t - expected).abs() > crate::ft::JITTER {
            let delay = (self.fir.len() / 2) as f64;
            self.started = true;
            self.raw = vec![0.0; self.fir.len() - 1];
            self.raw_t0 = t;
            self.raw_count = 0;
            self.history.clear();
            self.start = t - (delay * ns).round() as i128;
            self.pos = delay;
        }
        self.raw_count += samples.len() as u64;
        self.raw.extend_from_slice(samples);
        let taps = self.fir.len();
        let filtered = self.raw.len() + 1 - taps;
        self.history.extend((0..filtered).map(|i| {
            self.raw[i..i + taps]
                .iter()
                .zip(self.fir.iter().rev())
                .map(|(a, b)| a * b)
                .sum::<f32>()
        }));
        self.raw.drain(..filtered);
        let begin = self.start + (self.pos * ns).round() as i128;
        let mut out = Vec::new();
        while self.pos + 2.0 < self.history.len() as f64 {
            out.push(cubic(&self.history, self.pos) as f32);
            self.pos += self.step;
        }
        let drop = (self.pos as usize)
            .saturating_sub(1)
            .min(self.history.len());
        self.history.drain(..drop);
        self.pos -= drop as f64;
        self.start += (drop as f64 * ns).round() as i128;
        (out, begin)
    }
}

pub(crate) fn cubic(x: &[f32], p: f64) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let at = |i: isize| x[i.clamp(0, x.len() as isize - 1) as usize] as f64;
    let i = p.floor() as isize;
    let t = p - i as f64;
    let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|k| (std::f64::consts::TAU * hz * k as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn convert(x: &[f32], from: u32, to: u32) -> Vec<f32> {
        Input::new(from, to).convert(x, 0).0
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    #[test]
    fn cubic_interpolation_is_exact_on_lines_and_safe_on_edges() {
        let x: Vec<f32> = (0..10).map(|k| 3.0 * k as f32 + 1.0).collect();
        for p in [2.0, 2.25, 4.5, 6.75] {
            assert!((cubic(&x, p) - (3.0 * p + 1.0)).abs() < 1e-4);
        }
        assert_eq!(cubic(&[], 3.0), 0.0);
        assert!(cubic(&x, -5.0).is_finite() && cubic(&x, 50.0).is_finite());
    }

    #[test]
    fn resampling_keeps_frequency_and_length() {
        for (from, to) in [
            (12000, 48000),
            (48000, 12000),
            (12000, 44100),
            (44100, 12000),
        ] {
            let x = tone(1000.0, from, from as usize);
            let y = convert(&x, from, to);
            assert!(
                (y.len() as i64 - to as i64).abs() <= to as i64 / 100,
                "{from} {to}"
            );
            let tail = &y[to as usize / 10..];
            let crossings = tail
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count();
            let expected = 1000.0 * tail.len() as f64 / to as f64;
            assert!(
                (crossings as f64 - expected).abs() <= 2.0,
                "{from} {to} {crossings}"
            );
            assert!((rms(&y[to as usize / 10..to as usize * 9 / 10]) - 0.707).abs() < 0.02);
        }
    }

    #[test]
    fn downsampling_removes_content_above_nyquist() {
        let x = tone(9000.0, 48000, 48000);
        let mut input = Input::new(48000, 12000);
        let mut out = Vec::new();
        for (i, c) in x.chunks(960).enumerate() {
            out.extend(input.convert(c, i as i128 * 20_000_000).0);
        }
        assert!(rms(&out[1000..11000]) < 0.01);
        let x = tone(1500.0, 48000, 48000);
        let mut input = Input::new(48000, 12000);
        let (y, _) = input.convert(&x, 0);
        assert!((rms(&y[1000..11000]) - 0.707).abs() < 0.02);
    }

    #[test]
    fn streaming_conversion_keeps_time_stamps() {
        let x = tone(1000.0, 48000, 48000);
        let mut whole = Input::new(48000, 12000);
        let (a, t) = whole.convert(&x, 5_000_000_000);
        let mut parts = Input::new(48000, 12000);
        let mut b = Vec::new();
        let mut first = None;
        for (i, c) in x.chunks(1001).enumerate() {
            let (y, s) = parts.convert(
                c,
                5_000_000_000 + (i as i128 * 1001 * 1_000_000_000) / 48000,
            );
            first.get_or_insert(s);
            b.extend(y);
        }
        assert_eq!(first, Some(t));
        let n = a.len().min(b.len());
        assert!(n > 11_000);
        assert!(a[..n]
            .iter()
            .zip(&b[..n])
            .all(|(p, q)| (p - q).abs() < 1e-4));
        assert!((t - 5_000_000_000).abs() < 1_000_000);
    }
}
