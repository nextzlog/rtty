use crate::crc;
use crate::ldpc;
use crate::message;

pub const RATE: u32 = 12_000;
pub const START: f32 = 0.5;

#[derive(Debug, PartialEq)]
pub struct Protocol {
    pub name: &'static str,
    pub tones: usize,
    pub bits: usize,
    pub symbols: usize,
    pub sps: usize,
    pub bt: f32,
    pub slot: f32,
    pub sync: &'static [(usize, &'static [u8])],
    pub gray: &'static [u8],
    pub scramble: Option<u128>,
    pub decimation: usize,
}

pub const FT8: Protocol = Protocol {
    name: "FT8",
    tones: 8,
    bits: 3,
    symbols: 79,
    sps: 1920,
    bt: 2.0,
    slot: 15.0,
    sync: &[
        (0, &[3, 1, 4, 0, 6, 5, 2]),
        (36, &[3, 1, 4, 0, 6, 5, 2]),
        (72, &[3, 1, 4, 0, 6, 5, 2]),
    ],
    gray: &[0, 1, 3, 2, 5, 6, 4, 7],
    scramble: None,
    decimation: 60,
};

pub const FT4: Protocol = Protocol {
    name: "FT4",
    tones: 4,
    bits: 2,
    symbols: 105,
    sps: 576,
    bt: 1.0,
    slot: 7.5,
    sync: &[
        (1, &[0, 1, 3, 2]),
        (34, &[1, 0, 2, 3]),
        (67, &[2, 3, 1, 0]),
        (100, &[3, 2, 0, 1]),
    ],
    gray: &[0, 1, 3, 2],
    scramble: Some(0x4A5E89B4B08A7955BE28 >> 3),
    decimation: 18,
};

impl Protocol {
    pub fn spacing(&self) -> f32 {
        RATE as f32 / self.sps as f32
    }

    pub fn period(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.slot)
    }

    pub fn slot_samples(&self) -> usize {
        (self.slot * RATE as f32) as usize
    }

    pub fn data_positions(&self) -> Vec<usize> {
        let first = self.sync[0].0;
        let last = self.sync.last().map_or(0, |(p, c)| p + c.len());
        (first..last)
            .filter(|&i| {
                !self
                    .sync
                    .iter()
                    .any(|(p, c)| (*p..p + c.len()).contains(&i))
            })
            .collect()
    }

    pub fn tones_of(&self, payload: u128) -> Vec<u8> {
        let cw = ldpc::encode(crc::with_crc(payload ^ self.scramble.unwrap_or(0)));
        let mut t = vec![0u8; self.symbols];
        for (p, c) in self.sync {
            t[*p..p + c.len()].copy_from_slice(c);
        }
        for (k, i) in self.data_positions().into_iter().enumerate() {
            let v = cw[k * self.bits..(k + 1) * self.bits]
                .iter()
                .fold(0usize, |a, &b| a << 1 | b as usize);
            t[i] = self.gray[v];
        }
        t
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u8>, String> {
        Ok(self.tones_of(message::pack(text)?))
    }

    pub fn phases(&self, tones: &[u8], hz: f32) -> Vec<f32> {
        self.phases_at(tones, hz, RATE)
    }

    fn phases_at(&self, tones: &[u8], hz: f32, rate: u32) -> Vec<f32> {
        let dev = self.deviation(tones);
        let n = (dev.len() as u64 * rate as u64 / RATE as u64) as usize;
        let step = RATE as f64 / rate as f64;
        let w = 2.0 * std::f64::consts::PI / rate as f64;
        let spacing = self.spacing() as f64;
        let mut phi = 0f64;
        (0..n)
            .map(|k| {
                let d = crate::resample::cubic(&dev, k as f64 * step);
                let out = phi as f32;
                phi = (phi + w * (hz as f64 + spacing * d)) % (2.0 * std::f64::consts::PI);
                out
            })
            .collect()
    }

    fn deviation(&self, tones: &[u8]) -> Vec<f32> {
        let sps = self.sps;
        let n = tones.len();
        if n == 0 {
            return Vec::new();
        }
        let c = std::f32::consts::PI * (2.0 / std::f32::consts::LN_2).sqrt() * self.bt;
        let pulse = |t: f32| 0.5 * (erf(c * (t + 0.5)) - erf(c * (t - 0.5)));
        let table: Vec<f32> = (0..3 * sps)
            .map(|m| pulse((m as f32 + 0.5) / sps as f32 - 1.5))
            .collect();
        let mut dev = vec![0f32; n * sps];
        for i in 0..n + 2 {
            let tone = tones[i.saturating_sub(1).min(n - 1)] as f32;
            let start = i as isize * sps as isize - 2 * sps as isize;
            for (m, p) in table.iter().enumerate() {
                let k = start + m as isize;
                if (0..dev.len() as isize).contains(&k) {
                    dev[k as usize] += tone * p;
                }
            }
        }
        dev
    }

    pub fn envelope(&self, len: usize) -> Vec<f32> {
        let ramp = (self.sps / 8).max(1);
        (0..len)
            .map(|k| {
                let e = k.min(len.saturating_sub(1 + k));
                if e < ramp {
                    0.5 - 0.5 * (std::f32::consts::PI * e as f32 / ramp as f32).cos()
                } else {
                    1.0
                }
            })
            .collect()
    }

    pub fn waveform_at(&self, tones: &[u8], hz: f32, rate: u32) -> Vec<f32> {
        let p = self.phases_at(tones, hz, rate);
        let e = self.envelope(tones.len() * self.sps);
        let step = RATE as f64 / rate as f64;
        p.iter()
            .enumerate()
            .map(|(k, p)| crate::resample::cubic(&e, k as f64 * step) as f32 * p.sin())
            .collect()
    }
}

pub fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - t * (0.254_829_6
            + t * (-0.284_496_7 + t * (1.421_413_8 + t * (-1.453_152 + t * 1.061_405_4))))
            * (-x * x).exp();
    y.copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(p: &Protocol, text: &str) -> String {
        p.encode(text)
            .unwrap()
            .iter()
            .map(|t| char::from(b'0' + t))
            .collect()
    }

    #[test]
    fn tones_match_the_reference_vectors() {
        for (text, ft8, ft4) in crate::vectors::VECTORS {
            assert_eq!(tones(&FT8, text), *ft8, "FT8 {text}");
            assert_eq!(tones(&FT4, text), *ft4, "FT4 {text}");
        }
    }

    fn frequency(phases: &[f32], at: usize) -> f32 {
        let d = (phases[at + 1] - phases[at]).rem_euclid(std::f32::consts::TAU);
        d * RATE as f32 / std::f32::consts::TAU
    }

    #[test]
    fn the_waveform_has_the_right_length_tones_and_ramps() {
        for p in [&FT8, &FT4] {
            let t = p.encode("CQ JA1ZLO PM95").unwrap();
            let w = p.waveform_at(&t, 1000.0, RATE);
            assert_eq!(w.len(), p.symbols * p.sps);
            assert!(w.iter().all(|v| v.abs() <= 1.0));
            assert!(w[0].abs() < 1e-3 && w[w.len() - 1].abs() < 0.05);
            let ramp = p.sps / 8;
            let peak = w[ramp..3 * ramp].iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.95, "{peak}");
            let phases = p.phases(&t, 1000.0);
            for (s, &tone) in t.iter().enumerate() {
                let f = frequency(&phases, s * p.sps + p.sps / 2);
                let want = 1000.0 + tone as f32 * p.spacing();
                assert!(
                    (f - want).abs() < 0.2 * p.spacing(),
                    "{} {s} {f} {want}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn empty_input_is_safe() {
        assert!(FT8.phases(&[], 1000.0).is_empty());
        assert!(FT8.waveform_at(&[], 1000.0, RATE).is_empty());
        assert!(FT8.envelope(0).is_empty());
        assert_eq!(FT8.envelope(1).len(), 1);
        assert_eq!(FT4.waveform_at(&[2], 1000.0, RATE).len(), FT4.sps);
    }

    #[test]
    fn erf_is_accurate() {
        for (x, y) in [(0.0, 0.0), (0.5, 0.5205), (1.0, 0.8427), (2.0, 0.9953)] {
            assert!((erf(x) - y).abs() < 1e-3 && (erf(-x) + y).abs() < 1e-3);
        }
    }
}
