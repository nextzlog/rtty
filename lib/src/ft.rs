use std::time::{Duration, SystemTime};

use crate::engine::Decoder;
use crate::message::fields;
use crate::modem::{
    finite, nanos, time, Decode, Modem, Options, Parity, Transmission, Unit, MAX_HZ,
};
use crate::proto::{Protocol, FT4, FT8, RATE, START};
use crate::resample::Input;

const SECOND: i128 = 1_000_000_000;
const SAMPLE_NS: i128 = SECOND / RATE as i128;
pub(crate) const JITTER: i128 = 20_000_000;

pub(crate) struct Ft {
    proto: &'static Protocol,
    rate: u32,
    options: Options,
    decoder: Decoder,
    buffer: Vec<f32>,
    start: i128,
    input: Option<Input>,
}

pub struct Ft8(Ft);

pub struct Ft4(Ft);

impl Ft8 {
    pub fn new(rate: u32, options: Options) -> Result<Self, String> {
        Ft::new(&FT8, rate, options).map(Ft8)
    }
}

impl Ft4 {
    pub fn new(rate: u32, options: Options) -> Result<Self, String> {
        Ft::new(&FT4, rate, options).map(Ft4)
    }
}

macro_rules! modem {
    ($t:ty) => {
        impl Modem for $t {
            fn transmit(
                &mut self,
                text: &str,
                hz: f32,
                now: SystemTime,
            ) -> Result<Transmission, String> {
                self.0.transmit(text, hz, now)
            }

            fn push(&mut self, samples: &[f32], start: SystemTime) -> Vec<Decode> {
                self.0.push(samples, start)
            }

            fn options(&self) -> &Options {
                &self.0.options
            }

            fn set_options(&mut self, options: Options) {
                self.0.options = options.sanitized();
            }
        }
    };
}

modem!(Ft8);
modem!(Ft4);

impl Parity {
    pub fn of(slot: SystemTime, period: Duration) -> Parity {
        let period = period.as_nanos().max(1) as i128;
        if (nanos(slot) + SAMPLE_NS / 2).div_euclid(period) % 2 == 0 {
            Parity::Even
        } else {
            Parity::Odd
        }
    }
}

impl Ft {
    fn new(proto: &'static Protocol, rate: u32, options: Options) -> Result<Self, String> {
        if !(8000..=384_000).contains(&rate) {
            return Err(format!("unsupported sample rate {rate}"));
        }
        Ok(Ft {
            proto,
            rate,
            options: options.sanitized(),
            decoder: Decoder::new(proto),
            buffer: Vec::new(),
            start: 0,
            input: (rate != RATE).then(|| Input::new(rate, RATE)),
        })
    }

    fn period(&self) -> i128 {
        self.proto.period().as_nanos() as i128
    }

    fn transmit(&mut self, text: &str, hz: f32, now: SystemTime) -> Result<Transmission, String> {
        let width = self.proto.tones as f32 * self.proto.spacing();
        let top = MAX_HZ.min(self.rate as f32 / 2.0) - width;
        if !(hz.is_finite() && hz >= 0.0 && hz <= top) {
            return Err(format!("audio frequency {hz} Hz out of range"));
        }
        let audio = self
            .proto
            .waveform_at(&self.proto.encode(text)?, hz, self.rate);
        let period = self.period();
        let offset = (START as f64 * 1e9) as i128;
        let index = (nanos(now) - offset + period - 1).div_euclid(period);
        Ok(Transmission {
            audio,
            start: time(index * period + offset),
            parity: Some(if index.rem_euclid(2) == 0 {
                Parity::Even
            } else {
                Parity::Odd
            }),
        })
    }

    fn push(&mut self, samples: &[f32], start: SystemTime) -> Vec<Decode> {
        let samples = finite(samples);
        match self.input.as_mut() {
            Some(input) => {
                let (x, t) = input.convert(&samples, nanos(start));
                self.push_native(&x, t)
            }
            None => self.push_native(&samples, nanos(start)),
        }
    }

    fn push_native(&mut self, samples: &[f32], t: i128) -> Vec<Decode> {
        let end = self.start + self.buffer.len() as i128 * SECOND / RATE as i128;
        if self.buffer.is_empty() || (t - end).abs() > JITTER {
            self.buffer.clear();
            self.start = t;
        }
        self.buffer.extend_from_slice(samples);
        let period = self.period();
        let n = self.proto.slot_samples();
        let mut out = Vec::new();
        loop {
            let boundary = (self.start - SAMPLE_NS / 2 + period - 1).div_euclid(period) * period;
            let skip = ((boundary - self.start) * RATE as i128 + SECOND / 2).div_euclid(SECOND);
            let skip = skip.max(0) as usize;
            if skip + n > self.buffer.len() {
                let skip = skip.min(self.buffer.len());
                self.buffer.drain(..skip);
                self.start += skip as i128 * SECOND / RATE as i128;
                return out;
            }
            out.extend(self.decode_slot(skip, boundary));
            self.buffer.drain(..skip + n);
            self.start += (skip + n) as i128 * SECOND / RATE as i128;
        }
    }

    fn decode_slot(&self, skip: usize, slot: i128) -> Vec<Decode> {
        let n = self.proto.slot_samples();
        let at = time(slot + self.period());
        self.decoder
            .decode(&self.buffer[skip..skip + n], &self.options)
            .into_iter()
            .map(|s| Decode {
                slot: Some(time(slot)),
                at,
                snr: s.snr,
                hz: s.hz,
                dt: Some(s.dt),
                fields: fields(&s.text),
                text: s.text,
                unit: Unit::Message,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn at(secs: f64) -> SystemTime {
        if secs >= 0.0 {
            UNIX_EPOCH + Duration::from_secs_f64(secs)
        } else {
            UNIX_EPOCH - Duration::from_secs_f64(-secs)
        }
    }

    #[test]
    fn parity_follows_utc_slots_across_epoch_minute_and_hour() {
        let ft8 = Duration::from_secs(15);
        let ft4 = Duration::from_millis(7500);
        assert_eq!(Parity::of(UNIX_EPOCH, ft8), Parity::Even);
        assert_eq!(Parity::of(at(-15.0), ft8), Parity::Odd);
        assert_eq!(Parity::of(at(-30.0), ft8), Parity::Even);
        assert_eq!(Parity::of(at(1_799_999_985.0), ft8), Parity::Odd);
        assert_eq!(Parity::of(at(1_800_000_000.0), ft8), Parity::Even);
        assert_eq!(Parity::of(at(1_800_000_045.0), ft8), Parity::Odd);
        assert_eq!(Parity::of(at(1_800_000_007.5), ft4), Parity::Odd);
        assert_eq!(Parity::of(at(1_800_000_060.0), ft4), Parity::Even);
        assert_eq!(Parity::of(at(1_799_999_999.999_99), ft8), Parity::Even);
    }

    #[test]
    fn transmissions_start_in_the_next_slot_with_its_parity() {
        let mut ft8 = Ft8::new(12000, Options::default()).unwrap();
        let cases = [
            (1_800_000_001.0, 1_800_000_015.5, Parity::Odd),
            (1_800_000_000.2, 1_800_000_000.5, Parity::Even),
            (1_800_000_000.5, 1_800_000_000.5, Parity::Even),
            (1_799_999_999.0, 1_800_000_000.5, Parity::Even),
            (1_800_003_599.0, 1_800_003_600.5, Parity::Even),
            (-1.0, 0.5, Parity::Even),
            (-14.6, -14.5, Parity::Odd),
        ];
        for (now, start, parity) in cases {
            let tx = ft8.transmit("CQ JA1ZLO PM95", 1500.0, at(now)).unwrap();
            assert_eq!(tx.start, at(start), "{now}");
            assert_eq!(tx.parity, Some(parity), "{now}");
            assert_eq!(tx.audio.len(), 79 * 1920);
        }
        let mut ft4 = Ft4::new(48000, Options::default()).unwrap();
        let tx = ft4
            .transmit("CQ JA1ZLO PM95", 1500.0, at(1_800_000_001.0))
            .unwrap();
        assert_eq!(tx.start, at(1_800_000_008.0));
        assert_eq!(tx.parity, Some(Parity::Odd));
        assert_eq!(tx.audio.len(), 105 * 576 * 4);
    }

    #[test]
    fn transmit_level_is_flat_across_rates_and_frequencies() {
        let rms = |a: &[f32]| (a.iter().map(|v| v * v).sum::<f32>() / a.len() as f32).sqrt();
        for rate in [8000, 11025, 44100, 48000] {
            let mut ft8 = Ft8::new(rate, Options::default()).unwrap();
            let mut ft4 = Ft4::new(rate, Options::default()).unwrap();
            let top = MAX_HZ.min(rate as f32 / 2.0) - 90.0;
            for hz in [500.0, 0.5 * top, 0.8 * top, top] {
                for tx in [
                    ft8.transmit("CQ JA1ZLO PM95", hz, UNIX_EPOCH).unwrap(),
                    ft4.transmit("CQ JA1ZLO PM95", hz, UNIX_EPOCH).unwrap(),
                ] {
                    let level = rms(&tx.audio);
                    assert!((level - 0.7).abs() < 0.03, "{rate} {hz} {level}");
                }
            }
        }
    }

    #[test]
    fn bad_rates_frequencies_and_messages_are_errors() {
        assert!(Ft8::new(0, Options::default()).is_err());
        assert!(Ft4::new(1, Options::default()).is_err());
        let mut ft8 = Ft8::new(8000, Options::default()).unwrap();
        let now = UNIX_EPOCH;
        assert!(ft8.transmit("CQ JA1ZLO PM95", 3990.0, now).is_err());
        assert!(ft8.transmit("CQ JA1ZLO PM95", -5.0, now).is_err());
        assert!(ft8.transmit("CQ JA1ZLO PM95", f32::NAN, now).is_err());
        assert!(ft8
            .transmit("NOT A VALID MESSAGE AT ALL", 1500.0, now)
            .is_err());
        assert!(ft8.transmit("", 1500.0, now).is_err());
        assert!(ft8.transmit("CQ JA1ZLO PM95", 3900.0, now).is_ok());
    }

    #[test]
    fn a_high_max_hz_does_not_overrun_the_spectrum() {
        let o = Options {
            max_hz: 1e6,
            min_hz: 0.0,
            ..Options::default()
        };
        let mut ft8 = Ft8::new(12000, Options::default()).unwrap();
        ft8.0.options = o;
        assert!(ft8.push(&vec![0.01; 200_000], UNIX_EPOCH).is_empty());
        let mut ft4 = Ft4::new(12000, Options::default()).unwrap();
        ft4.0.options = o;
        assert!(ft4.push(&vec![0.01; 100_000], UNIX_EPOCH).is_empty());
    }
}
