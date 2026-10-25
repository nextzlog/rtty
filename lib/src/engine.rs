use std::f32::consts::PI;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::crc;
use crate::ldpc;
use crate::message;
use crate::proto::{Protocol, RATE, START};
use crate::Options;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Signal {
    pub text: String,
    pub hz: f32,
    pub dt: f32,
    pub snr: f32,
    pub tones: Vec<u8>,
}

fn near_rx(opts: &Options, hz: f32) -> bool {
    opts.rx_hz
        .is_some_and(|rx| (hz - rx).abs() <= opts.ft.rx_width)
}

const SUB: usize = 32;
const FADE: usize = 2;
const TIME_OS: usize = 4;

pub(crate) struct Decoder {
    proto: &'static Protocol,
    len: usize,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    stft: Arc<dyn Fft<f32>>,
}

struct Candidate {
    hz: f32,
    start: f32,
    score: f32,
}

impl Decoder {
    pub fn new(proto: &'static Protocol) -> Self {
        let mut planner = FftPlanner::new();
        let len = proto.slot_samples();
        Decoder {
            proto,
            len,
            forward: planner.plan_fft_forward(len),
            inverse: planner.plan_fft_inverse(len / proto.decimation),
            stft: planner.plan_fft_forward(2 * proto.sps),
        }
    }

    pub fn decode(&self, samples: &[f32], opts: &Options) -> Vec<Signal> {
        let mut x: Vec<f32> = samples.iter().copied().take(self.len).collect();
        x.resize(self.len, 0.0);
        if opts.ft.blanker {
            blank(&mut x);
        }
        let mut found: Vec<Signal> = Vec::new();
        for _ in 0..opts.ft.passes {
            let spectrum = self.spectrum(&x);
            let mut new = Vec::new();
            for c in self.candidates(&x, opts) {
                if found.iter().chain(&new).any(|s: &Signal| {
                    (s.hz - c.hz).abs() < self.proto.spacing()
                        && (s.dt + START - c.start).abs() < 0.1
                }) {
                    continue;
                }
                if let Some(s) = self.try_candidate(&spectrum, &c, opts) {
                    if !found.iter().chain(&new).any(|f| f.text == s.text) {
                        new.push(s);
                    }
                }
            }
            if new.is_empty() {
                break;
            }
            for s in &new {
                self.subtract(&mut x, s);
            }
            found.extend(new);
        }
        found.sort_by_key(|s| !near_rx(opts, s.hz));
        found
    }

    fn spectrum(&self, x: &[f32]) -> Vec<Complex32> {
        let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
        self.forward.process(&mut buf);
        buf
    }

    fn candidates(&self, x: &[f32], opts: &Options) -> Vec<Candidate> {
        let p = self.proto;
        let sps = p.sps;
        let hop = sps / TIME_OS;
        let nfft = 2 * sps;
        let frames = (x.len() - sps) / hop + 1;
        let bin_hz = RATE as f32 / nfft as f32;
        let max_hz = opts
            .max_hz
            .min(RATE as f32 / 2.0 - (2 * p.tones + 4) as f32 * bin_hz);
        let min_hz = opts.min_hz.clamp(0.0, max_hz);
        let nbins = (max_hz / bin_hz) as usize + 2 * p.tones + 2;
        let mut power = vec![0f32; frames * nbins];
        let mut buf = vec![Complex32::default(); nfft];
        for f in 0..frames {
            for (b, v) in buf.iter_mut().enumerate() {
                *v = Complex32::new(if b < sps { x[f * hop + b] } else { 0.0 }, 0.0);
            }
            self.stft.process(&mut buf);
            for b in 0..nbins {
                power[f * nbins + b] = buf[b].norm_sqr();
            }
        }
        let lo = (min_hz / bin_hz) as usize;
        let hi = (max_hz / bin_hz) as usize;
        let nominal = (START * RATE as f32 / hop as f32) as isize;
        let reach = (2.0 * RATE as f32 / hop as f32) as isize;
        let mut all = Vec::new();
        for b in lo..hi {
            let mut best: Vec<(f32, isize)> = Vec::new();
            for t in nominal - reach / 2..nominal + reach {
                let mut sig = 0.0;
                let mut tot = 0.0;
                for (pos, costas) in p.sync {
                    for (k, &tone) in costas.iter().enumerate() {
                        let fr = t + ((pos + k) * TIME_OS) as isize;
                        if fr < 0 || fr >= frames as isize {
                            continue;
                        }
                        let row = &power[fr as usize * nbins + b..];
                        sig += row[2 * tone as usize];
                        tot += (0..p.tones).map(|j| row[2 * j]).sum::<f32>();
                    }
                }
                if tot > 0.0 {
                    best.push((sig / ((tot - sig) / (p.tones - 1) as f32), t));
                }
            }
            best.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut kept: Vec<(f32, isize)> = Vec::new();
            for c in best {
                if kept.len() >= 2 {
                    break;
                }
                if kept.iter().all(|k| (k.1 - c.1).abs() > TIME_OS as isize) {
                    kept.push(c);
                }
            }
            for (score, t) in kept {
                all.push(Candidate {
                    hz: b as f32 * bin_hz,
                    start: (t * hop as isize) as f32 / RATE as f32,
                    score,
                });
            }
        }
        all.sort_by(|a, b| {
            near_rx(opts, b.hz)
                .cmp(&near_rx(opts, a.hz))
                .then(b.score.total_cmp(&a.score))
        });
        all.retain(|c| c.score > opts.ft.threshold);
        let mut out: Vec<Candidate> = Vec::new();
        for c in all {
            if out.len() >= opts.ft.candidates {
                break;
            }
            if !out
                .iter()
                .any(|o| (o.hz - c.hz).abs() < bin_hz * 1.1 && (o.start - c.start).abs() < 0.07)
            {
                out.push(c);
            }
        }
        out
    }

    fn baseband(&self, spectrum: &[Complex32], hz: f32) -> (Vec<Complex32>, f32) {
        let p = self.proto;
        let m = self.len / p.decimation;
        let bin_hz = RATE as f32 / self.len as f32;
        let center = (hz / bin_hz).round() as isize;
        let lo = -1.5 * p.spacing();
        let hi = (p.tones as f32 + 0.5) * p.spacing();
        let taper = p.spacing();
        let mut buf = vec![Complex32::default(); m];
        for (i, v) in buf.iter_mut().enumerate() {
            let k = if i < m / 2 {
                i as isize
            } else {
                i as isize - m as isize
            };
            let f = k as f32 * bin_hz;
            let w = if f < lo || f > hi {
                0.0
            } else {
                let e = (f - lo).min(hi - f);
                if e < taper {
                    0.5 - 0.5 * (PI * e / taper).cos()
                } else {
                    1.0
                }
            };
            let src = center + k;
            if w > 0.0 && src > 0 && (src as usize) < self.len / 2 {
                *v = spectrum[src as usize] * w;
            }
        }
        self.inverse.process(&mut buf);
        let scale = 1.0 / (m as f32).sqrt() / (self.len as f32).sqrt();
        buf.iter_mut().for_each(|v| *v *= scale);
        (buf, center as f32 * bin_hz)
    }

    fn try_candidate(
        &self,
        spectrum: &[Complex32],
        c: &Candidate,
        opts: &Options,
    ) -> Option<Signal> {
        let p = self.proto;
        let (z, center) = self.baseband(spectrum, c.hz);
        let fs = RATE as f32 / p.decimation as f32;
        let coarse = (c.start * fs).round() as isize;
        let spacing = p.spacing();
        let mut best = (f32::MIN, 0isize, 0f32);
        let tone = |f: f32| -> Vec<Complex32> {
            (0..SUB)
                .map(|n| {
                    let ph = -2.0 * PI * f * n as f32 / fs;
                    Complex32::new(ph.cos(), ph.sin())
                })
                .collect()
        };
        let symbol = |i0: isize, s: usize, r: &[Complex32], z: &[Complex32]| -> Complex32 {
            let base = i0 + (s * SUB) as isize;
            if base >= 0 && base as usize + SUB <= z.len() {
                let w = &z[base as usize..base as usize + SUB];
                w.iter().zip(r).map(|(a, b)| a * b).sum()
            } else {
                Complex32::default()
            }
        };
        let offset = c.hz - center;
        let steps: isize = 5;
        let refs: Vec<Vec<Vec<Complex32>>> = (-steps..=steps)
            .map(|d| {
                let df = offset + d as f32 * spacing / 16.0;
                (0..p.tones)
                    .map(|j| tone(df + j as f32 * spacing))
                    .collect()
            })
            .collect();
        let reach = (SUB / TIME_OS) as isize - 2;
        for di in -reach..=reach {
            let i0 = coarse + di;
            for (d, r) in refs.iter().enumerate() {
                let mut s = 0.0;
                for (pos, costas) in p.sync {
                    for (k, &t) in costas.iter().enumerate() {
                        s += symbol(i0, pos + k, &r[t as usize], &z).norm_sqr();
                    }
                }
                if s > best.0 {
                    best = (s, i0, offset + (d as isize - steps) as f32 * spacing / 16.0);
                }
            }
        }
        let (_, i0, df) = best;
        let shift: Vec<Complex32> = (0..z.len())
            .map(|k| {
                let ph = -2.0 * PI * df * k as f32 / fs;
                z[k] * Complex32::new(ph.cos(), ph.sin())
            })
            .collect();
        let base: Vec<Vec<Complex32>> = (0..p.tones).map(|j| tone(j as f32 * spacing)).collect();
        let mut power = vec![vec![0f32; p.tones]; p.symbols];
        for (s, row) in power.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = symbol(i0, s, &base[j], &shift).norm();
            }
        }
        let hits = p
            .sync
            .iter()
            .flat_map(|(pos, costas)| costas.iter().enumerate().map(move |(k, &t)| (pos + k, t)))
            .filter(|&(s, t)| {
                let row = &power[s];
                (0..p.tones).all(|j| row[j] <= row[t as usize])
            })
            .count();
        if hits < opts.ft.min_sync {
            return None;
        }
        let mut inv = vec![0usize; p.tones];
        for (v, &g) in p.gray.iter().enumerate() {
            inv[g as usize] = v;
        }
        let (mut on, mut off, mut no, mut nf) = (0f32, 0f32, 0f32, 0f32);
        for (pos, costas) in p.sync {
            for (k, &t) in costas.iter().enumerate() {
                for (j, a) in power[pos + k].iter().enumerate() {
                    if j == t as usize {
                        on += a * a;
                        no += 1.0;
                    } else {
                        off += a * a;
                        nf += 1.0;
                    }
                }
            }
        }
        let sigma2 = off / nf;
        let amp = (on / no - sigma2).max(0.05 * sigma2).sqrt();
        let peak: Vec<f32> = power
            .iter()
            .map(|r| r.iter().fold(0f32, |m, v| m.max(v * v)))
            .collect();
        let local: Vec<f32> = (0..p.symbols)
            .map(|s| {
                let w = &peak[s.saturating_sub(FADE)..(s + FADE + 1).min(p.symbols)];
                let e = w.iter().sum::<f32>() / w.len() as f32;
                (e - sigma2).max(0.05 * sigma2).sqrt()
            })
            .collect();
        let llr_with = |amp: &dyn Fn(usize) -> f32| {
            let mut llr = [0f32; ldpc::N];
            for (k, s) in p.data_positions().into_iter().enumerate() {
                let m: Vec<f32> = power[s]
                    .iter()
                    .map(|r| ln_i0(2.0 * amp(s) * r / sigma2))
                    .collect();
                for b in 0..p.bits {
                    let mask = 1 << (p.bits - 1 - b);
                    let (mut one, mut zero) = (Vec::new(), Vec::new());
                    for (tone, &v) in m.iter().enumerate() {
                        if inv[tone] & mask != 0 {
                            one.push(v)
                        } else {
                            zero.push(v)
                        }
                    }
                    llr[k * p.bits + b] = 1.4 * (lse(&one) - lse(&zero));
                }
            }
            llr
        };
        let cw = ldpc::decode(&llr_with(&|_| amp), opts.ft.iterations).or_else(|| {
            let lo = local.iter().fold(f32::MAX, |m, v| m.min(*v));
            let hi = local.iter().fold(0f32, |m, v| m.max(*v));
            (opts.ft.fading && hi > 2.0 * lo)
                .then(|| ldpc::decode(&llr_with(&|s| local[s]), opts.ft.iterations))
                .flatten()
        })?;
        let a91 = ldpc::bits_to_a91(&cw);
        if !crc::crc_ok(a91) {
            return None;
        }
        let payload = (a91 >> 14) ^ p.scramble.unwrap_or(0);
        let text = message::unpack(payload)?;
        let tones = p.tones_of(payload);
        let mut sig = 0.0;
        let mut noise = 0.0;
        for (s, row) in power.iter().enumerate() {
            for (j, a) in row.iter().enumerate() {
                if j == tones[s] as usize {
                    sig += a * a;
                } else {
                    noise += a * a;
                }
            }
        }
        let ns = p.symbols as f32;
        let noise = noise / (ns * (p.tones - 1) as f32);
        let sig = (sig / ns - noise).max(noise * 1e-3);
        let bandwidth = fs / SUB as f32;
        let snr = 10.0 * (sig / noise).log10() - 10.0 * (2500.0 / bandwidth).log10();
        Some(Signal {
            text,
            hz: center + df,
            dt: i0 as f32 / fs - START,
            snr: (snr * 10.0).round() / 10.0,
            tones,
        })
    }

    fn subtract(&self, x: &mut [f32], s: &Signal) {
        let p = self.proto;
        let phases = p.phases(&s.tones, s.hz);
        let env = p.envelope(phases.len());
        let start = ((s.dt + START) * RATE as f32).round() as isize;
        let r: Vec<Complex32> = phases
            .iter()
            .zip(&env)
            .map(|(&ph, &e)| Complex32::new(ph.cos(), ph.sin()) * e)
            .collect();
        let mut prod = vec![Complex32::default(); r.len()];
        for (k, v) in prod.iter_mut().enumerate() {
            let i = start + k as isize;
            if i >= 0 && (i as usize) < x.len() {
                *v = x[i as usize] * r[k].conj();
            }
        }
        let w = p.sps;
        let mut smooth = vec![Complex32::default(); r.len()];
        let mut acc = Complex32::default();
        let mut cum = vec![Complex32::default(); r.len() + 1];
        for k in 0..r.len() {
            acc += prod[k];
            cum[k + 1] = acc;
        }
        for (k, v) in smooth.iter_mut().enumerate() {
            let a = k.saturating_sub(w / 2);
            let b = (k + w / 2).min(r.len());
            *v = (cum[b] - cum[a]) * (2.0 / (b - a) as f32);
        }
        for k in 0..r.len() {
            let i = start + k as isize;
            if i >= 0 && (i as usize) < x.len() {
                x[i as usize] -= (smooth[k] * r[k]).re;
            }
        }
    }
}

fn lse(v: &[f32]) -> f32 {
    let m = v.iter().copied().fold(f32::MIN, f32::max);
    m + v.iter().map(|x| (x - m).exp()).sum::<f32>().ln()
}

fn ln_i0(x: f32) -> f32 {
    if x < 3.75 {
        let t = (x / 3.75).powi(2);
        (1.0 + t
            * (3.515_623
                + t * (3.089_942
                    + t * (1.206_749 + t * (0.265_973 + t * (0.036_076_8 + t * 0.004_581_3))))))
            .ln()
    } else {
        x - 0.5 * (2.0 * PI * x).ln() + (1.0 + 0.125 / x).ln()
    }
}

fn blank(x: &mut [f32]) {
    let win = (RATE / 100) as usize;
    let mut rms: Vec<f32> = x
        .chunks(win)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let level = {
        let mut sorted = rms.clone();
        sorted.sort_by(f32::total_cmp);
        sorted[sorted.len() / 2]
    };
    if level <= 0.0 {
        return;
    }
    let limit = 6.0 * level;
    rms.iter_mut().for_each(|r| *r = 0.0);
    let guard = (RATE / 1000) as usize;
    let hits: Vec<usize> = (0..x.len()).filter(|&i| x[i].abs() > limit).collect();
    for i in hits {
        let lo = i.saturating_sub(guard);
        let hi = (i + guard).min(x.len());
        x[lo..hi].iter_mut().for_each(|v| *v = 0.0);
    }
}
