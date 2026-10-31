mod demodulator;
mod modulator;
mod params;

use std::time::SystemTime;

pub(crate) use crate::afsk::demodulator::Demodulator;
pub(crate) use crate::afsk::modulator::Modulator;
pub(crate) use crate::afsk::params::Params;
use crate::baudot::Decoder;
use crate::modem::{finite, nanos, time, Decode, Modem, Options, Transmission, Unit, MAX_HZ};

const SHORT: usize = 2;
const LONG: usize = 64;
const MARGIN: f64 = 0.1;

pub struct Rtty {
    rate: u32,
    options: Options,
    params: Params,
    active: bool,
    demodulator: Demodulator,
    decoder: Decoder,
    word: String,
    level: f64,
    line: Option<f32>,
    last: Option<i128>,
    idle: usize,
    clock: Option<(i128, u64)>,
}

impl Rtty {
    pub fn new(rate: u32, options: Options) -> Result<Self, String> {
        if !(8000..=384_000).contains(&rate) {
            return Err(format!("unsupported sample rate {rate}"));
        }
        let options = options.sanitized();
        let params = Params::new(&options.rtty, options.rx_hz);
        Ok(Rtty {
            rate,
            options,
            params,
            active: false,
            demodulator: Demodulator::new(params, rate),
            decoder: Decoder::new(params.uos),
            word: String::new(),
            level: 0.0,
            line: None,
            last: None,
            idle: 0,
            clock: None,
        }
        .tuned())
    }

    fn tuned(mut self) -> Self {
        let (lo, hi) = (
            self.params.mark.min(self.params.space),
            self.params.mark.max(self.params.space),
        );
        self.active = lo >= self.options.min_hz as f64
            && hi <= self.options.max_hz as f64
            && hi < self.rate as f64 / 2.0;
        self
    }

    fn flush(&mut self, out: &mut Vec<Decode>) {
        if self.word.is_empty() {
            return;
        }
        let at = time(self.last.unwrap_or(0));
        let chars = self.word.chars().count();
        let level = (self.level / chars.max(1) as f64).clamp(0.01, 0.99);
        if chars <= SHORT && level < self.params.squelch + MARGIN {
            self.word.clear();
            self.level = 0.0;
            return;
        }
        let snr = (10.0 * (level / (1.0 - level)).log10()) as f32;
        self.line = Some(snr);
        out.push(Decode {
            at,
            snr,
            hz: self.params.mark.min(self.params.space) as f32,
            text: std::mem::take(&mut self.word),
            unit: Unit::Word,
        });
        self.level = 0.0;
    }

    fn feed(&mut self, out: &mut Vec<Decode>) {
        self.flush(out);
        if let Some(snr) = self.line.take() {
            out.push(Decode {
                at: time(self.last.unwrap_or(0)),
                snr,
                hz: self.params.mark.min(self.params.space) as f32,
                text: "\n".into(),
                unit: Unit::LineBreak,
            });
        }
    }
}

impl Modem for Rtty {
    fn transmit(&mut self, text: &str, hz: f32, now: SystemTime) -> Result<Transmission, String> {
        let o = &self.options.rtty;
        let top = MAX_HZ.min(self.rate as f32 / 2.0) - o.shift;
        if !(hz.is_finite() && hz > 0.0 && hz <= top) {
            return Err(format!("audio frequency {hz} Hz out of range"));
        }
        let params = Params::new(o, Some(hz));
        Ok(Transmission {
            audio: Modulator::new(params, self.rate).modulate(text),
            start: now,
        })
    }

    fn push(&mut self, samples: &[f32], start: SystemTime) -> Vec<Decode> {
        let samples = finite(samples);
        let t0 = nanos(start);
        let ns = 1e9 / self.rate as f64;
        let mut out = Vec::new();
        if let Some((base, count)) = self.clock {
            let expected = base + (count as f64 * ns).round() as i128;
            if (t0 - expected).abs() > crate::modem::JITTER {
                self.flush(&mut out);
                self.clock = None;
            }
        }
        let (base, count) = *self.clock.get_or_insert((t0, 0));
        self.clock = Some((base, count + samples.len() as u64));
        if !self.active {
            return out;
        }
        let gap = (3.0 * 7.5 * self.params.bit_len(self.rate)) as usize;
        for (k, &x) in samples.iter().enumerate() {
            if let Some(code) = self.demodulator.push(x) {
                let level = self.demodulator.level;
                if let Some(c) = self.decoder.decode(code) {
                    self.last = Some(base + ((count + k as u64) as f64 * ns).round() as i128);
                    self.idle = 0;
                    if c == '\n' {
                        self.feed(&mut out);
                    } else if c == ' ' || c == '\r' {
                        self.flush(&mut out);
                    } else {
                        self.word.push(c);
                        self.level += level;
                        if self.word.len() >= LONG {
                            self.flush(&mut out);
                        }
                    }
                }
            }
            self.idle += 1;
            if self.idle == gap {
                self.flush(&mut out);
            }
        }
        out
    }

    fn options(&self) -> &Options {
        &self.options
    }

    fn set_options(&mut self, options: Options) {
        if let Ok(rtty) = Rtty::new(self.rate, options) {
            *self = Rtty {
                clock: self.clock,
                ..rtty
            };
        }
    }
}
