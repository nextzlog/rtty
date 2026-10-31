use std::time::{Duration, SystemTime};

use crate::sender::{Output, Sender, Timing};
use crate::{Decode, Modem, Options, Rtty, RttyOptions};

#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub at: SystemTime,
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Burst {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / self.rate.max(1) as f64)
    }

    pub fn end(&self) -> SystemTime {
        self.at + self.duration()
    }

    pub fn lead_in(&self, now: SystemTime) -> Vec<f32> {
        let wait = self.at.duration_since(now).unwrap_or_default();
        let n = (wait.as_secs_f64() * self.rate as f64).round() as usize;
        let mut audio = vec![0.0; n];
        audio.extend_from_slice(&self.samples);
        audio
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub callsign: String,
    pub hz: f32,
    pub dial_hz: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Start,
    Stop,
    Options(RttyOptions),
    Timing(Timing),
    Send {
        text: String,
        at: SystemTime,
    },
    Halt,
    Samples {
        rate: u32,
        samples: Vec<f32>,
        at: SystemTime,
    },
    OutputRate(u32),
    Tick(SystemTime),
    PttLead(Option<Duration>),
    Keyed(bool),
    KeyFailed(String),
    Dial(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    Running(bool),
    Decode(Decode),
    Busy(bool),
    Burst(Burst),
    Halt,
    Ptt(bool),
    Dial(u64),
    Error(String),
}

pub struct Engine {
    options: Options,
    timing: Timing,
    rx: Option<(u32, Rtty)>,
    sender: Sender,
    running: bool,
    busy: bool,
    out: Vec<Notice>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            options: Options::default(),
            timing: Timing::default(),
            rx: None,
            sender: Sender::new(48000),
            running: false,
            busy: false,
            out: Vec::new(),
        }
    }

    pub fn options(&self) -> &RttyOptions {
        &self.options.rtty
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn take(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.out)
    }

    pub fn handle(&mut self, c: Command) {
        match c {
            Command::Start => {
                if !self.running {
                    self.running = true;
                    self.out.push(Notice::Running(true));
                }
            }
            Command::Stop => {
                if self.running {
                    self.running = false;
                    self.rx = None;
                    self.out.push(Notice::Running(false));
                }
            }
            Command::Options(o) => {
                self.options = Options {
                    rtty: o,
                    ..self.options
                }
                .sanitized();
            }
            Command::Timing(t) => self.timing = t,
            Command::Send { text, at } => self.sender.send(&text, self.timing, at),
            Command::Halt => {
                if self.sender.abort() {
                    self.out.push(Notice::Halt);
                }
            }
            Command::Samples { rate, samples, at } => self.feed(rate, &samples, at),
            Command::OutputRate(rate) => self.sender.rate = rate,
            Command::PttLead(lead) => self.sender.host = lead.is_some(),
            Command::Keyed(on) => {
                if let Err(e) = self.sender.keyed(on, self.options) {
                    self.out.push(Notice::Error(e));
                }
            }
            Command::KeyFailed(e) => {
                if self.sender.abort() {
                    self.out.push(Notice::Halt);
                }
                self.out.push(Notice::Error(e));
            }
            Command::Dial(hz) => self.out.push(Notice::Dial(hz)),
            Command::Tick(now) => {
                if let Err(e) = self.sender.tick(now, self.timing, self.options) {
                    self.out.push(Notice::Error(e));
                }
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        for o in self.sender.take() {
            self.out.push(match o {
                Output::Burst(b) => Notice::Burst(b),
                Output::Ptt(on) => Notice::Ptt(on),
            });
        }
        if self.sender.busy() != self.busy {
            self.busy = !self.busy;
            self.out.push(Notice::Busy(self.busy));
        }
    }

    fn feed(&mut self, rate: u32, samples: &[f32], at: SystemTime) {
        if !self.running {
            return;
        }
        let o = self.options;
        if !self.rx.as_ref().is_some_and(|(r, _)| *r == rate) {
            match Rtty::new(rate, o) {
                Ok(m) => self.rx = Some((rate, m)),
                Err(e) => {
                    self.out.push(Notice::Error(e));
                    return;
                }
            }
        }
        let (_, m) = self.rx.as_mut().expect("opened");
        if *m.options() != o {
            m.set_options(o);
        }
        for d in m.push(samples, at) {
            self.out.push(Notice::Decode(d));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn a_burst_waits_with_silence_and_ends_after_its_samples() {
        let at = UNIX_EPOCH + Duration::from_millis(15500);
        let b = Burst {
            at,
            rate: 12000,
            samples: vec![1.0; 12000],
        };
        let audio = b.lead_in(at - Duration::from_millis(600));
        assert_eq!(audio.len(), 7200 + 12000);
        assert!(audio[..7200].iter().all(|v| *v == 0.0) && audio[7200] == 1.0);
        assert_eq!(b.lead_in(at + Duration::from_secs(1)).len(), 12000);
        assert_eq!(b.end(), at + Duration::from_secs(1));
    }

    #[test]
    fn sent_text_comes_back_as_bursts_and_decodes() {
        let t0 = UNIX_EPOCH;
        let mut tx = Engine::new();
        tx.handle(Command::OutputRate(12000));
        tx.handle(Command::Send {
            text: "CQ TEST JA1ZLO\r\n".into(),
            at: t0,
        });
        assert_eq!(tx.take(), [Notice::Ptt(true), Notice::Busy(true)]);
        tx.handle(Command::Tick(t0 + Duration::from_secs(1)));
        let n = tx.take();
        let Some(Notice::Burst(b)) = n.first() else {
            panic!("{n:?}")
        };
        let mut rx = Engine::new();
        rx.handle(Command::Samples {
            rate: 12000,
            samples: b.samples.clone(),
            at: b.at,
        });
        assert!(rx.take().is_empty(), "stopped");
        rx.handle(Command::Start);
        rx.handle(Command::Samples {
            rate: 12000,
            samples: b.samples.clone(),
            at: b.at,
        });
        rx.handle(Command::Samples {
            rate: 12000,
            samples: vec![0.0; 12000],
            at: b.end(),
        });
        let words: Vec<String> = rx
            .take()
            .into_iter()
            .filter_map(|n| match n {
                Notice::Decode(d) if d.unit == crate::Unit::Word => Some(d.text),
                _ => None,
            })
            .collect();
        assert_eq!(words, ["CQ", "TEST", "JA1ZLO"]);
        tx.handle(Command::Halt);
        assert_eq!(
            tx.take(),
            [Notice::Halt, Notice::Ptt(false), Notice::Busy(false)]
        );
    }

    #[test]
    fn a_failed_keyer_stops_and_the_dial_comes_back() {
        let t0 = UNIX_EPOCH;
        let mut e = Engine::new();
        e.handle(Command::PttLead(Some(Duration::from_millis(200))));
        e.handle(Command::Send {
            text: "CQ".into(),
            at: t0,
        });
        assert_eq!(e.take(), [Notice::Ptt(true), Notice::Busy(true)]);
        e.handle(Command::KeyFailed("no CAT".into()));
        assert_eq!(
            e.take(),
            [
                Notice::Halt,
                Notice::Error("no CAT".into()),
                Notice::Ptt(false),
                Notice::Busy(false)
            ]
        );
        e.handle(Command::Dial(14_080_000));
        assert_eq!(e.take(), [Notice::Dial(14_080_000)]);
    }
}
