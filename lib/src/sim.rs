use std::f64::consts::TAU;
use std::time::{Duration, Instant, UNIX_EPOCH};

use rustfft::num_complex::Complex64;
use rustfft::FftPlanner;

use crate::afsk::{Modulator, Params};
use crate::modem::{Modem, RttyOptions};

pub const NOISE: f32 = 0.05;
pub const BANDWIDTH: f32 = 2500.0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Channel {
    pub drift: f32,
    pub shift: f32,
    pub spread: f32,
    pub delay: f32,
    pub qsb_hz: f32,
    pub qsb_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Awgn,
    MidQuiet,
    MidModerate,
    MidDisturbed,
    LowModerate,
    LowDisturbed,
    HighModerate,
    HighDisturbed,
    Flutter,
}

impl Preset {
    pub const ALL: [Preset; 9] = [
        Preset::Awgn,
        Preset::MidQuiet,
        Preset::MidModerate,
        Preset::MidDisturbed,
        Preset::LowModerate,
        Preset::LowDisturbed,
        Preset::HighModerate,
        Preset::HighDisturbed,
        Preset::Flutter,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Awgn => "awgn",
            Preset::MidQuiet => "mid-quiet",
            Preset::MidModerate => "mid-moderate",
            Preset::MidDisturbed => "mid-disturbed",
            Preset::LowModerate => "low-moderate",
            Preset::LowDisturbed => "low-disturbed",
            Preset::HighModerate => "high-moderate",
            Preset::HighDisturbed => "high-disturbed",
            Preset::Flutter => "flutter",
        }
    }

    pub fn channel(self) -> Channel {
        let (spread, delay) = match self {
            Preset::Awgn => return Channel::default(),
            Preset::MidQuiet => (0.1, 0.5),
            Preset::MidModerate => (0.5, 1.0),
            Preset::MidDisturbed => (1.0, 2.0),
            Preset::LowModerate => (1.5, 2.0),
            Preset::LowDisturbed => (10.0, 6.0),
            Preset::HighModerate => (10.0, 3.0),
            Preset::HighDisturbed => (30.0, 7.0),
            Preset::Flutter => (20.0, 2.0),
        };
        Channel::watterson(spread, delay)
    }
}

impl Channel {
    pub fn watterson(spread: f32, delay: f32) -> Self {
        Channel {
            spread,
            delay,
            ..Channel::default()
        }
    }

    pub fn apply(&self, wave: &[f32], rate: u32, seed: u64) -> Vec<f64> {
        impair(wave, rate, self, &mut Rng::new(seed))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub text: String,
    pub hz: f32,
    pub dt: f32,
    pub snr: f32,
    pub channel: Channel,
}

impl Station {
    pub fn new(text: &str, hz: f32, dt: f32, snr: f32) -> Self {
        Station {
            text: text.into(),
            hz,
            dt,
            snr,
            channel: Channel::default(),
        }
    }

    pub fn with(mut self, channel: Channel) -> Self {
        self.channel = channel;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Qrn {
    pub rate: f32,
    pub level: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Agc {
    pub depth_db: f32,
    pub hz: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub rate: u32,
    pub seconds: f32,
    pub noise: f32,
    pub rate_error_ppm: f64,
    pub clock_offset: f32,
    pub qrn: Qrn,
    pub agc: Agc,
}

impl Band {
    pub fn audio(rate: u32, seconds: f32) -> Self {
        Band {
            rate,
            seconds,
            noise: NOISE,
            rate_error_ppm: 0.0,
            clock_offset: 0.0,
            qrn: Qrn::default(),
            agc: Agc::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Source {
    Rtty(RttyOptions),
    Tone,
}

impl Source {
    fn wave(&self, text: &str, hz: f32, band: &Band) -> Result<(Vec<f32>, f32), String> {
        let rate = band.rate;
        match self {
            Source::Rtty(o) => Ok((
                Modulator::new(Params::new(o, Some(hz)), rate).modulate(text),
                0.0,
            )),
            _ => {
                let n = (band.seconds * rate as f32) as usize;
                let w = TAU * hz as f64 / rate as f64;
                Ok(((0..n).map(|k| (w * k as f64).sin() as f32).collect(), 0.0))
            }
        }
    }
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn gauss(&mut self) -> f64 {
        let u = self.uniform().max(1e-300);
        let v = self.uniform();
        (-2.0 * u.ln()).sqrt() * (TAU * v).cos()
    }

    fn complex(&mut self) -> Complex64 {
        Complex64::new(self.gauss(), self.gauss()) * std::f64::consts::FRAC_1_SQRT_2
    }
}

pub fn amplitude(wave: &[f32], snr: f32, noise: f32, rate: u32) -> f64 {
    let power = wave.iter().map(|w| (*w as f64).powi(2)).sum::<f64>() / wave.len().max(1) as f64;
    let per_hz = (noise * noise) as f64 / (rate as f64 / 2.0);
    let target = 10f64.powf(snr as f64 / 10.0) * per_hz * BANDWIDTH as f64;
    (target / power.max(1e-12)).sqrt()
}

pub fn render(
    band: &Band,
    source: Source,
    stations: &[Station],
    seed: u64,
) -> Result<Vec<f32>, String> {
    let rate = band.rate;
    let len = (band.seconds * rate as f32) as usize;
    let mut rng = Rng::new(seed);
    let mut x: Vec<f64> = (0..len).map(|_| band.noise as f64 * rng.gauss()).collect();
    for (i, s) in stations.iter().enumerate() {
        let (wave, lead) = source.wave(&s.text, s.hz, band)?;
        let amp = amplitude(&wave, s.snr, band.noise, rate);
        let mut chan = Rng::new(seed.wrapping_mul(31).wrapping_add(i as u64 + 1));
        let y = impair(&wave, rate, &s.channel, &mut chan);
        let start = ((s.dt + lead + band.clock_offset) * rate as f32).round() as isize;
        for (k, v) in y.iter().enumerate() {
            let j = start + k as isize;
            if j >= 0 && (j as usize) < len {
                x[j as usize] += amp * v;
            }
        }
    }
    qrn(&mut x, band, &mut Rng::new(seed ^ 0x0051_524e));
    agc(&mut x, band);
    let x = if band.rate_error_ppm != 0.0 {
        stretch(&x, 1.0 + band.rate_error_ppm * 1e-6)
    } else {
        x
    };
    Ok(x.into_iter().map(|v| v as f32).collect())
}

fn qrn(x: &mut [f64], band: &Band, rng: &mut Rng) {
    if band.qrn.rate <= 0.0 || x.is_empty() {
        return;
    }
    let fs = band.rate as f64;
    let mut t = 0.0;
    loop {
        t += -rng.uniform().max(1e-12).ln() / band.qrn.rate as f64;
        let start = (t * fs) as usize;
        if start >= x.len() {
            return;
        }
        let peak = band.qrn.level as f64 * band.noise as f64 * rng.uniform().max(0.05).powf(-0.7);
        let decay = 0.5e-3 * fs * (0.5 + rng.uniform());
        for k in 0..(6.0 * decay) as usize {
            if let Some(v) = x.get_mut(start + k) {
                *v += peak * (-(k as f64) / decay).exp() * rng.gauss();
            }
        }
    }
}

fn agc(x: &mut [f64], band: &Band) {
    if band.agc.depth_db <= 0.0 || band.agc.hz <= 0.0 {
        return;
    }
    let fs = band.rate as f64;
    let low = 10f64.powf(-band.agc.depth_db as f64 / 20.0);
    let mut g = 1.0;
    for (k, v) in x.iter_mut().enumerate() {
        let keyed = (k as f64 / fs * band.agc.hz as f64).fract() < 0.5;
        let target = if keyed { low } else { 1.0 };
        let tau = if target < g { 0.002 } else { 0.3 };
        g += (target - g) / (tau * fs);
        *v *= g;
    }
}

fn impair(wave: &[f32], rate: u32, c: &Channel, rng: &mut Rng) -> Vec<f64> {
    if *c == Channel::default() || wave.is_empty() {
        return wave.iter().map(|&w| w as f64).collect();
    }
    let a = analytic(wave);
    let n = a.len();
    let fs = rate as f64;
    let delay = (c.delay as f64 * 1e-3 * fs).round() as usize;
    let paths = if c.delay > 0.0 { 2 } else { 1 };
    let gains: Vec<Vec<Complex64>> = (0..paths)
        .map(|_| fading(n, fs, c.spread as f64, rng))
        .collect();
    let scale = (1.0 / paths as f64).sqrt();
    let qsb_phase = TAU * rng.uniform();
    (0..n)
        .map(|k| {
            let t = k as f64 / fs;
            let mut v = gains[0][k] * a[k];
            if paths == 2 && k >= delay {
                v += gains[1][k] * a[k - delay];
            }
            let phase = std::f64::consts::PI * c.drift as f64 * t * t + TAU * c.shift as f64 * t;
            let depth =
                c.qsb_db as f64 * (0.5 - 0.5 * (TAU * c.qsb_hz as f64 * t + qsb_phase).cos());
            (v * Complex64::from_polar(1.0, phase) * scale).re * 10f64.powf(-depth / 20.0)
        })
        .collect()
}

fn analytic(wave: &[f32]) -> Vec<Complex64> {
    let n = wave.len();
    let mut planner = FftPlanner::new();
    let mut buf: Vec<Complex64> = wave
        .iter()
        .map(|&w| Complex64::new(w as f64, 0.0))
        .collect();
    planner.plan_fft_forward(n).process(&mut buf);
    for (k, v) in buf.iter_mut().enumerate() {
        if k == 0 || 2 * k == n {
            continue;
        }
        *v *= if 2 * k < n { 2.0 } else { 0.0 };
    }
    planner.plan_fft_inverse(n).process(&mut buf);
    buf.iter().map(|v| v / n as f64).collect()
}

fn fading(n: usize, fs: f64, spread: f64, rng: &mut Rng) -> Vec<Complex64> {
    if spread <= 0.0 {
        return vec![Complex64::new(1.0, 0.0); n];
    }
    let rate = (20.0 * spread).max(50.0);
    let sigma = rate / (TAU * spread / 2.0);
    let half = (3.0 * sigma).ceil() as usize;
    let kernel: Vec<f64> = (0..=2 * half)
        .map(|i| (-((i as f64 - half as f64) / sigma).powi(2) / 2.0).exp())
        .collect();
    let m = (n as f64 / fs * rate).ceil() as usize + 2;
    let white: Vec<Complex64> = (0..m + 2 * half).map(|_| rng.complex()).collect();
    let g: Vec<Complex64> = (0..m)
        .map(|i| kernel.iter().zip(&white[i..]).map(|(k, w)| w * k).sum())
        .collect();
    let rms = (g.iter().map(|v| v.norm_sqr()).sum::<f64>() / m as f64).sqrt();
    (0..n)
        .map(|k| {
            let p = k as f64 / fs * rate;
            let i = (p as usize).min(m - 2);
            let f = p - i as f64;
            (g[i] * (1.0 - f) + g[i + 1] * f) / rms
        })
        .collect()
}

pub fn stretch(x: &[f64], ratio: f64) -> Vec<f64> {
    if x.is_empty() {
        return Vec::new();
    }
    let at = |i: isize| x[i.clamp(0, x.len() as isize - 1) as usize];
    (0..x.len())
        .map(|k| {
            let p = k as f64 / ratio;
            let i = p.floor() as isize;
            let t = p - i as f64;
            let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            b + 0.5
                * t
                * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RttyTrial {
    pub sent: usize,
    pub errors: usize,
    pub text: String,
    pub elapsed: Duration,
}

pub fn rtty_trial(
    modem: &mut dyn Modem,
    band: &Band,
    station: &Station,
    seed: u64,
) -> Result<RttyTrial, String> {
    let options = *modem.options();
    let audio = render(
        band,
        Source::Rtty(options.rtty),
        std::slice::from_ref(station),
        seed,
    )?;
    modem.set_options(crate::Options {
        rx_hz: Some(station.hz),
        ..options
    });
    let clock = Instant::now();
    let mut got = modem.push(&audio, UNIX_EPOCH);
    let end = UNIX_EPOCH + Duration::from_secs_f64(audio.len() as f64 / band.rate as f64);
    got.extend(modem.push(&vec![0f32; band.rate as usize], end));
    let elapsed = clock.elapsed();
    modem.set_options(options);
    let text = words(got.iter().map(|d| d.text.as_str()));
    let want = words(station.text.split_whitespace());
    Ok(RttyTrial {
        sent: want.chars().count(),
        errors: edit_distance(&want, &text),
        text,
        elapsed,
    })
}

pub fn words<'a>(w: impl Iterator<Item = &'a str>) -> String {
    w.collect::<Vec<_>>().join(" ")
}

pub fn edit_distance(want: &str, got: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (want.chars().collect(), got.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let keep = prev + (a[i - 1] != b[j - 1]) as usize;
            prev = row[j];
            row[j] = keep.min(row[j] + 1).min(row[j - 1] + 1);
        }
    }
    row[b.len()]
}
