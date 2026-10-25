use crate::afsk::Params;
use crate::baudot::{Encoder, LTRS};

const IDLE_BEFORE: f64 = 0.15;
const IDLE_AFTER: f64 = 0.1;

const AMPLITUDE: f32 = 0.5;

pub(crate) struct Modulator {
    params: Params,
    rate: u32,
    phase: f64,
    clock: f64,
    sent: usize,
}

impl Modulator {
    pub fn new(params: Params, rate: u32) -> Self {
        Modulator {
            params,
            rate: rate.max(1),
            phase: 0.0,
            clock: 0.0,
            sent: 0,
        }
    }

    fn tone(&mut self, mark: bool, bits: f64, out: &mut Vec<f32>) {
        let hz = if mark {
            self.params.mark
        } else {
            self.params.space
        };
        self.clock += bits * self.params.bit_len(self.rate);
        let end = self.clock.round() as usize;
        let step = std::f64::consts::TAU * hz / self.rate as f64;
        while self.sent < end {
            out.push(AMPLITUDE * self.phase.sin() as f32);
            self.phase = (self.phase + step) % std::f64::consts::TAU;
            self.sent += 1;
        }
    }

    fn code(&mut self, code: u8, out: &mut Vec<f32>) {
        self.tone(false, 1.0, out);
        for bit in 0..5 {
            self.tone(code >> bit & 1 == 1, 1.0, out);
        }
        self.tone(true, 1.5, out);
    }

    pub fn modulate(&mut self, text: &str) -> Vec<f32> {
        let mut out = Vec::new();
        let baud = self.params.baud;
        let idle = |s: f64| s * baud;
        self.tone(true, idle(IDLE_BEFORE), &mut out);
        let mut codes = Encoder::new(self.params.uos).encode(text);
        if codes.first() != Some(&LTRS) {
            codes.insert(0, LTRS);
        }
        for c in codes {
            self.code(c, &mut out);
        }
        self.tone(true, idle(IDLE_AFTER), &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_lasts_seven_and_a_half_bits() {
        let p = Params::default();
        let mut a = Modulator::new(p, 8000);
        let one = a.modulate("E").len();
        let mut a = Modulator::new(p, 8000);
        let two = a.modulate("EE").len();
        let char_len = 7.5 * 8000.0 / 45.45;
        assert!(((two - one) as f64 - char_len).abs() <= 1.0);
        assert!(!a.modulate("").is_empty());
    }

    #[test]
    fn the_tones_stay_in_level_and_phase() {
        let mut a = Modulator::new(Params::default(), 48000);
        let s = a.modulate("RYRYRY");
        assert!(s.iter().all(|x| x.abs() <= AMPLITUDE + 1e-6));
        let max = (std::f64::consts::TAU * 2295.0 / 48000.0) as f32 * AMPLITUDE * 1.01;
        assert!(s.windows(2).all(|w| (w[1] - w[0]).abs() <= max));
    }
}
