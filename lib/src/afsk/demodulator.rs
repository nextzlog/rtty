use std::collections::VecDeque;

use crate::afsk::Params;

struct Tone {
    step: f64,
    phase: f64,
    window: VecDeque<(f64, f64)>,
    sum: (f64, f64),
    len: usize,
}

impl Tone {
    fn new(hz: f64, rate: u32, len: usize) -> Self {
        Tone {
            step: std::f64::consts::TAU * hz / rate as f64,
            phase: 0.0,
            window: VecDeque::with_capacity(len + 1),
            sum: (0.0, 0.0),
            len: len.max(1),
        }
    }

    fn push(&mut self, x: f32) -> f64 {
        let (s, c) = self.phase.sin_cos();
        self.phase = (self.phase + self.step) % std::f64::consts::TAU;
        let v = (x as f64 * c, -(x as f64) * s);
        self.window.push_back(v);
        self.sum.0 += v.0;
        self.sum.1 += v.1;
        if self.window.len() > self.len {
            let old = self.window.pop_front().unwrap_or_default();
            self.sum.0 -= old.0;
            self.sum.1 -= old.1;
        }
        self.sum.0.hypot(self.sum.1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Mark(usize),
    Ready,
    Char { start: f64, bit: u32, code: u8 },
}

struct Envelope {
    value: f64,
    decay: f64,
}

impl Envelope {
    fn new(len: f64) -> Self {
        Envelope {
            value: 0.0,
            decay: 1.0 / len.max(1.0),
        }
    }

    fn follow(&mut self, x: f64) -> f64 {
        self.value = if x > self.value {
            x
        } else {
            self.value + (x - self.value) * self.decay
        };
        self.value
    }
}

struct Blanker {
    mean: f64,
    hold: usize,
    run: usize,
    len: usize,
}

impl Blanker {
    fn new(rate: u32) -> Self {
        Blanker {
            mean: 0.0,
            hold: 0,
            run: 0,
            len: (rate / 1000) as usize,
        }
    }

    fn push(&mut self, x: f32) -> f32 {
        let a = (x as f64).abs();
        if self.mean > 0.0 && a > 5.0 * self.mean {
            self.hold = self.len;
        }
        if self.hold > 0 && self.run < 8 * self.len {
            self.hold -= 1;
            self.run += 1;
            return 0.0;
        }
        if self.run >= 8 * self.len {
            self.mean = a.max(self.mean);
        }
        self.hold = 0;
        self.run = 0;
        self.mean += (a - self.mean) * 2e-3;
        x
    }
}

const ATC: f64 = 16.0;
const CARRIER: f64 = 3.0;
const BALANCE: f64 = 2.0;
const SPREAD: f64 = 6.0;

pub(crate) struct Demodulator {
    mark: Tone,
    space: Tone,
    mark_env: Envelope,
    space_env: Envelope,
    blanker: Option<Blanker>,
    atc: bool,
    bit_len: f64,
    clock: usize,
    state: State,
    clarity: f64,
    spaces: usize,
    floor: f64,
    carrier: f64,
    peak: f64,
    slow: f64,
    pub squelch: f64,
    pub level: f64,
}

impl Demodulator {
    pub fn new(params: Params, rate: u32) -> Self {
        let bit_len = params.bit_len(rate);
        let len = bit_len.round() as usize;
        Demodulator {
            mark: Tone::new(params.mark, rate, len),
            space: Tone::new(params.space, rate, len),
            mark_env: Envelope::new(ATC * bit_len),
            space_env: Envelope::new(ATC * bit_len),
            blanker: params.blanker.then(|| Blanker::new(rate)),
            atc: params.atc,
            bit_len,
            clock: 0,
            state: State::Mark(0),
            clarity: 0.0,
            spaces: 0,
            floor: 0.0,
            carrier: 0.0,
            peak: 0.0,
            slow: 1.0 / (rate as f64 * 2.0),
            squelch: params.squelch,
            level: 0.0,
        }
    }

    pub fn push(&mut self, x: f32) -> Option<u8> {
        let x = match self.blanker.as_mut() {
            Some(b) => b.push(x),
            None => x,
        };
        let m = self.mark.push(x);
        let s = self.space.push(x);
        let clarity = (m - s).abs() / (m + s).max(1e-9);
        let lo = m.min(s);
        self.floor += (lo - self.floor) * if lo < self.floor { 0.01 } else { self.slow };
        let carrier = m.max(s) / self.floor.max(1e-12);
        let pm = self.mark_env.follow(m);
        let ps = self.space_env.follow(s);
        let balanced = pm < BALANCE * ps && ps < BALANCE * pm;
        let (m, s) = if self.atc && !balanced && pm > 0.0 && ps > 0.0 {
            let floor = 0.35 * pm.max(ps);
            (m / pm.max(floor), s / ps.max(floor))
        } else {
            (m, s)
        };
        let mark = m >= s;
        let now = self.clock as f64;
        self.clock += 1;
        match self.state {
            State::Mark(n) => {
                let n = if mark { n + 1 } else { 0 };
                self.state = if n as f64 >= self.bit_len / 2.0 {
                    State::Ready
                } else {
                    State::Mark(n)
                };
            }
            State::Ready if !mark => {
                self.state = State::Char {
                    start: now - self.bit_len / 2.0,
                    bit: 0,
                    code: 0,
                };
                self.clarity = 0.0;
                self.carrier = 0.0;
                self.spaces = 0;
            }
            State::Ready => {}
            State::Char { start, bit, code } if now >= start + (bit + 1) as f64 * self.bit_len => {
                self.clarity += clarity;
                if bit == 0 {
                    self.carrier = carrier;
                }
                self.peak = if bit == 0 {
                    carrier
                } else {
                    self.peak.max(carrier)
                };
                match bit {
                    0 if mark
                        || carrier < CARRIER
                        || (self.spaces as f64) < 0.35 * self.bit_len =>
                    {
                        self.state = State::Ready
                    }
                    0..=5 => {
                        let code = match bit {
                            0 => code,
                            _ => code | (mark as u8) << (bit - 1),
                        };
                        self.state = State::Char {
                            start,
                            bit: bit + 1,
                            code,
                        };
                    }
                    _ => {
                        let level = self.clarity / 7.0;
                        if mark
                            && level >= self.squelch
                            && self.carrier * SPREAD >= self.peak
                            && carrier * SPREAD >= self.carrier
                        {
                            self.level = level;
                            self.state = State::Ready;
                            return Some(code);
                        }
                        self.state = State::Mark(0);
                    }
                }
            }
            State::Char { bit: 0, .. } => self.spaces += !mark as usize,
            State::Char { .. } => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::afsk::Modulator;

    fn demodulate(demodulator: &mut Demodulator, samples: &[f32]) -> String {
        let mut d = crate::baudot::Decoder::new(true);
        samples
            .iter()
            .filter_map(|&x| demodulator.push(x))
            .filter_map(|c| d.decode(c))
            .collect()
    }

    struct Noise(u64);

    impl Noise {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }

        fn gauss(&mut self) -> f64 {
            let (a, b) = (self.uniform(), self.uniform());
            (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
        }
    }

    fn through_noise(text: &str, p: Params, rate: u32, snr_db: f64, seed: u64) -> String {
        let mut signal = Modulator::new(p, rate).modulate(text);
        let mut noise = Noise(seed);
        let power = 0.25 / 2.0;
        let sigma = (power / 10f64.powf(snr_db / 10.0) * rate as f64 / 2.0 / 3000.0).sqrt();
        for x in signal.iter_mut() {
            *x += (sigma * noise.gauss()) as f32;
        }
        let mut demodulator = Demodulator::new(p, rate);
        demodulate(&mut demodulator, &signal)
    }

    fn errors(want: &str, got: &str) -> usize {
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

    const TEXT: &str = "CQ TEST JA1ZLO JA1ZLO TEST 599-13M 599 100110M RYRYRY $1.5 (A/B)?\r\n";

    #[test]
    fn clean_audio_round_trips_at_common_rates_and_speeds() {
        for rate in [8000, 11025, 44100, 48000] {
            for baud in [45.45, 50.0, 75.0] {
                let p = Params {
                    baud,
                    ..Params::default()
                };
                assert_eq!(through_noise(TEXT, p, rate, 60.0, 1), TEXT, "{rate} {baud}");
            }
        }
    }

    #[test]
    fn reversed_and_other_tones() {
        let reversed = Params {
            mark: 2295.0,
            space: 2125.0,
            ..Params::default()
        };
        assert_eq!(through_noise(TEXT, reversed, 48000, 60.0, 2), TEXT);
        let low = Params {
            mark: 1275.0,
            space: 1445.0,
            ..Params::default()
        };
        assert_eq!(through_noise(TEXT, low, 48000, 60.0, 3), TEXT);
    }

    #[test]
    fn a_wrong_shift_or_reversal_does_not_decode() {
        let sent = Params::default();
        let mut signal = Modulator::new(sent, 48000).modulate(TEXT);
        signal.extend(Modulator::new(sent, 48000).modulate(TEXT));
        let reversed = Params {
            mark: sent.space,
            space: sent.mark,
            ..sent
        };
        let got = demodulate(&mut Demodulator::new(reversed, 48000), &signal);
        assert!(errors(TEXT, &got) > TEXT.len() / 2, "{got:?}");
    }

    #[test]
    fn noise_costs_few_characters_at_moderate_snr() {
        let text = TEXT.repeat(3);
        let mut report = Vec::new();
        let limits = [(20.0, 0), (10.0, 0), (6.0, 0), (3.0, 2), (0.0, 10)];
        let report_only = [(-3.0, usize::MAX), (-6.0, usize::MAX)];
        for (snr, most) in limits.into_iter().chain(report_only) {
            let bad: usize = (0..3)
                .map(|seed| {
                    let got = through_noise(&text, Params::default(), 12000, snr, 7 + seed);
                    errors(&text, &got)
                })
                .sum();
            report.push((snr, bad));
            assert!(bad <= most.saturating_mul(3), "{report:?}");
        }
        eprintln!(
            "character errors in 3 x {} at SNR (dB, 3 kHz): {report:?}",
            text.len()
        );
    }

    #[test]
    fn noise_alone_prints_little() {
        let mut noise = Noise(99);
        let samples: Vec<f32> = (0..48000 * 10)
            .map(|_| (0.1 * noise.gauss()) as f32)
            .collect();
        let got = demodulate(&mut Demodulator::new(Params::default(), 48000), &samples);
        assert!(got.len() <= 3, "{got:?}");
    }

    #[test]
    fn decoding_starts_in_the_middle_of_a_transmission() {
        let signal = Modulator::new(Params::default(), 48000).modulate(TEXT);
        let got = demodulate(
            &mut Demodulator::new(Params::default(), 48000),
            &signal[signal.len() / 3..],
        );
        let tail = &TEXT[TEXT.len() / 2..];
        assert!(got.ends_with(tail), "{got:?}");
    }
}
