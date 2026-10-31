use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub trait Modem {
    fn transmit(&mut self, text: &str, hz: f32, now: SystemTime) -> Result<Transmission, String>;
    fn push(&mut self, samples: &[f32], start: SystemTime) -> Vec<Decode>;
    fn options(&self) -> &Options;
    fn set_options(&mut self, options: Options);
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decode {
    pub at: SystemTime,
    pub snr: f32,
    pub hz: f32,
    pub text: String,
    pub unit: Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Message,
    Word,
    LineBreak,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transmission {
    pub audio: Vec<f32>,
    pub start: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub min_hz: f32,
    pub max_hz: f32,
    pub rx_hz: Option<f32>,
    pub rtty: RttyOptions,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RttyOptions {
    pub mark: f32,
    pub shift: f32,
    pub baud: f32,
    pub reverse: bool,
    pub uos: bool,
    pub squelch: f32,
    pub atc: bool,
    pub blanker: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min_hz: 100.0,
            max_hz: 3000.0,
            rx_hz: None,
            rtty: RttyOptions::default(),
        }
    }
}

impl Default for RttyOptions {
    fn default() -> Self {
        RttyOptions {
            mark: 2125.0,
            shift: 170.0,
            baud: 45.45,
            reverse: false,
            uos: true,
            squelch: 0.55,
            atc: true,
            blanker: true,
        }
    }
}

pub(crate) const MAX_HZ: f32 = 5500.0;

pub(crate) const JITTER: i128 = 20_000_000;

impl Options {
    pub fn check(&self) -> Result<(), String> {
        let ok = |c: bool, what: &str| if c { Ok(()) } else { Err(what.to_string()) };
        ok(self.min_hz.is_finite() && self.min_hz >= 0.0, "min_hz")?;
        ok(
            self.max_hz.is_finite() && self.max_hz > self.min_hz && self.max_hz <= MAX_HZ,
            "max_hz",
        )?;
        ok(self.rx_hz.is_none_or(|f| f.is_finite()), "rx_hz")?;
        let r = &self.rtty;
        ok(r.mark.is_finite() && r.mark > 0.0, "rtty.mark")?;
        ok(r.shift.is_finite() && r.shift > 0.0, "rtty.shift")?;
        ok(
            r.baud.is_finite() && (10.0..=300.0).contains(&r.baud),
            "rtty.baud",
        )?;
        ok((0.0..1.0).contains(&r.squelch), "rtty.squelch")
    }

    pub fn sanitized(self) -> Options {
        let d = Options::default();
        let fin = |v: f32, or: f32| if v.is_finite() { v } else { or };
        let min_hz = fin(self.min_hz, d.min_hz).clamp(0.0, MAX_HZ - 1.0);
        let max_hz = fin(self.max_hz, d.max_hz).clamp(min_hz + 1.0, MAX_HZ);
        let r = self.rtty;
        Options {
            min_hz,
            max_hz,
            rx_hz: self.rx_hz.filter(|v| v.is_finite()),
            rtty: RttyOptions {
                mark: fin(r.mark, d.rtty.mark).clamp(1.0, MAX_HZ),
                shift: fin(r.shift, d.rtty.shift).clamp(1.0, MAX_HZ),
                baud: fin(r.baud, d.rtty.baud).clamp(10.0, 300.0),
                squelch: fin(r.squelch, d.rtty.squelch).clamp(0.0, 0.99),
                ..r
            },
        }
    }
}

pub(crate) fn nanos(t: SystemTime) -> i128 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}

pub(crate) fn time(nanos: i128) -> SystemTime {
    let d = |n: u128| Duration::new((n / 1_000_000_000) as u64, (n % 1_000_000_000) as u32);
    if nanos >= 0 {
        UNIX_EPOCH + d(nanos as u128)
    } else {
        UNIX_EPOCH - d(nanos.unsigned_abs())
    }
}

pub(crate) fn finite(samples: &[f32]) -> std::borrow::Cow<'_, [f32]> {
    if samples.iter().all(|x| x.is_finite()) {
        samples.into()
    } else {
        samples
            .iter()
            .map(|&x| if x.is_finite() { x } else { 0.0 })
            .collect::<Vec<_>>()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_checked_and_sanitized() {
        assert_eq!(Options::default().check(), Ok(()));
        let bad = [
            Options {
                min_hz: -1.0,
                ..Options::default()
            },
            Options {
                max_hz: 50.0,
                ..Options::default()
            },
            Options {
                max_hz: 9000.0,
                ..Options::default()
            },
            Options {
                min_hz: f32::NAN,
                ..Options::default()
            },
            Options {
                rx_hz: Some(f32::INFINITY),
                ..Options::default()
            },
            Options {
                rtty: RttyOptions {
                    baud: 0.0,
                    ..RttyOptions::default()
                },
                ..Options::default()
            },
            Options {
                rtty: RttyOptions {
                    shift: -170.0,
                    ..RttyOptions::default()
                },
                ..Options::default()
            },
        ];
        for o in bad {
            assert!(o.check().is_err(), "{o:?}");
            assert_eq!(o.sanitized().check(), Ok(()), "{o:?}");
        }
        assert_eq!(Options::default().sanitized(), Options::default());
    }

    #[test]
    fn time_conversion_is_exact_around_the_epoch_and_far_away() {
        for n in [
            0i128,
            1,
            -1,
            999_999_999,
            -1_000_000_001,
            1_800_000_000_123_456_789,
            40_000_000_000_000_000_000,
            -40_000_000_000_000_000_000,
        ] {
            assert_eq!(nanos(time(n)), n);
        }
    }

    #[test]
    fn non_finite_samples_become_silence() {
        assert!(matches!(finite(&[1.0, 2.0]), std::borrow::Cow::Borrowed(_)));
        assert_eq!(
            &*finite(&[f32::NAN, 1.0, f32::NEG_INFINITY]),
            &[0.0, 1.0, 0.0]
        );
    }
}
