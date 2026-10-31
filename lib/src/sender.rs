use std::time::{Duration, SystemTime};

use crate::engine::Burst;
use crate::{Modem, Options, Rtty};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub ptt: bool,
    pub before: Duration,
    pub after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            ptt: true,
            before: Duration::from_millis(300),
            after: Duration::from_millis(300),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Lead(SystemTime),
    Keying,
    Release,
    Playing(SystemTime),
    Tail(SystemTime),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Output {
    Burst(Burst),
    Ptt(bool),
}

#[derive(Debug)]
pub(crate) struct Sender {
    pub rate: u32,
    pub host: bool,
    state: State,
    now: SystemTime,
    queue: String,
    ptt_on: bool,
    out: Vec<Output>,
}

impl Sender {
    pub fn new(rate: u32) -> Self {
        Sender {
            rate,
            host: false,
            state: State::Idle,
            now: SystemTime::UNIX_EPOCH,
            queue: String::new(),
            ptt_on: false,
            out: Vec::new(),
        }
    }

    pub fn busy(&self) -> bool {
        self.state != State::Idle
    }

    pub fn send(&mut self, text: &str, t: Timing, now: SystemTime) {
        self.queue.push_str(text);
        self.now = now;
        if self.state == State::Idle && self.host {
            self.ptt(true);
            self.state = State::Keying;
        } else if self.state == State::Idle {
            if t.ptt {
                self.ptt(true);
            }
            self.state = State::Lead(now + if t.ptt { t.before } else { Duration::ZERO });
        }
    }

    pub fn abort(&mut self) -> bool {
        let was = self.busy();
        self.queue.clear();
        self.ptt(false);
        self.state = State::Idle;
        was
    }

    pub fn keyed(&mut self, on: bool, o: Options) -> Result<(), String> {
        match self.state {
            State::Keying if on => self.begin(o, self.now),
            State::Release if !on && self.queue.is_empty() => {
                self.state = State::Idle;
                Ok(())
            }
            State::Release if !on => {
                self.ptt(true);
                self.state = State::Keying;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn ptt(&mut self, on: bool) {
        if self.ptt_on != on {
            self.ptt_on = on;
            self.out.push(Output::Ptt(on));
        }
    }

    fn begin(&mut self, o: Options, now: SystemTime) -> Result<(), String> {
        let text = std::mem::take(&mut self.queue);
        let r = Rtty::new(self.rate, o).and_then(|mut m| m.transmit(&text, o.rtty.mark, now));
        match r {
            Ok(tx) => {
                let b = Burst {
                    at: now,
                    rate: self.rate,
                    samples: tx.audio,
                };
                self.state = State::Playing(b.end());
                self.out.push(Output::Burst(b));
                Ok(())
            }
            Err(e) => {
                self.abort();
                Err(e)
            }
        }
    }

    pub fn tick(&mut self, now: SystemTime, t: Timing, o: Options) -> Result<(), String> {
        self.now = now;
        match self.state {
            State::Lead(at) if now >= at => self.begin(o, now)?,
            State::Playing(end) if now >= end => {
                if !self.queue.is_empty() {
                    self.begin(o, now)?;
                } else if self.host {
                    self.ptt(false);
                    self.state = State::Release;
                } else if t.ptt && !t.after.is_zero() {
                    self.state = State::Tail(now + t.after);
                } else {
                    self.ptt(false);
                    self.state = State::Idle;
                }
            }
            State::Tail(_) if !self.queue.is_empty() => self.begin(o, now)?,
            State::Tail(at) if now >= at => {
                self.ptt(false);
                self.state = State::Idle;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn take(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(samples: &[f32]) -> String {
        let mut rx = Rtty::new(8000, Options::default()).unwrap();
        let mut words: Vec<String> = rx
            .push(samples, SystemTime::UNIX_EPOCH)
            .into_iter()
            .map(|d| d.text)
            .collect();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs_f64(samples.len() as f64 / 8000.0);
        words.extend(rx.push(&[0.0; 8000], at).into_iter().map(|d| d.text));
        words.join(" ")
    }

    fn timing(ptt: bool) -> Timing {
        Timing {
            ptt,
            before: Duration::from_millis(100),
            after: Duration::from_millis(50),
        }
    }

    fn played(out: &[Output]) -> Vec<f32> {
        out.iter()
            .flat_map(|o| match o {
                Output::Burst(b) => b.samples.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ptt_goes_on_before_and_off_after_the_text() {
        let mut s = Sender::new(8000);
        let (t, o) = (timing(true), Options::default());
        let t0 = SystemTime::UNIX_EPOCH;
        s.send("CQ TEST JA1ZLO", t, t0);
        assert_eq!(s.take(), [Output::Ptt(true)]);
        s.tick(t0 + ms(50), t, o).unwrap();
        assert!(s.take().is_empty());
        s.tick(t0 + ms(100), t, o).unwrap();
        let out = s.take();
        let Output::Burst(b) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(b.at, t0 + ms(100));
        assert_eq!(heard(&played(&out)), "CQ TEST JA1ZLO");
        let end = b.end();
        s.tick(end - ms(1), t, o).unwrap();
        s.tick(end, t, o).unwrap();
        assert!(s.take().is_empty() && s.busy());
        s.tick(end + ms(50), t, o).unwrap();
        assert_eq!(s.take(), [Output::Ptt(false)]);
        assert!(!s.busy());
    }

    #[test]
    fn text_sent_meanwhile_follows_in_the_same_transmission() {
        let mut s = Sender::new(8000);
        let (t, o) = (timing(true), Options::default());
        let t0 = SystemTime::UNIX_EPOCH;
        s.send("TU ", t, t0);
        s.tick(t0 + ms(100), t, o).unwrap();
        s.send("QRZ", t, t0);
        s.tick(t0 + Duration::from_secs(5), t, o).unwrap();
        let out = s.take();
        assert_eq!(out.iter().filter(|o| **o == Output::Ptt(true)).count(), 1);
        assert_eq!(heard(&played(&out)), "TU QRZ");
    }

    #[test]
    fn without_ptt_the_text_starts_at_once() {
        let mut s = Sender::new(8000);
        let t0 = SystemTime::UNIX_EPOCH;
        let (t, o) = (timing(false), Options::default());
        s.send("RYRY", t, t0);
        s.tick(t0, t, o).unwrap();
        assert!(matches!(s.take()[..], [Output::Burst(_)]));
        s.tick(t0 + Duration::from_secs(5), t, o).unwrap();
        assert!(!s.busy() && s.take().is_empty());
    }

    #[test]
    fn abort_and_errors_turn_ptt_off() {
        let mut s = Sender::new(8000);
        let (t, o) = (timing(true), Options::default());
        let t0 = SystemTime::UNIX_EPOCH;
        s.send("CQ", t, t0);
        s.tick(t0 + Duration::from_secs(1), t, o).unwrap();
        assert!(s.abort());
        assert!(!s.busy());
        assert_eq!(s.take().last(), Some(&Output::Ptt(false)));
        assert!(!s.abort());
        let mut s = Sender::new(1000);
        s.send("CQ", t, t0);
        assert!(s.tick(t0 + Duration::from_secs(1), t, o).is_err());
        assert_eq!(s.take(), [Output::Ptt(true), Output::Ptt(false)]);
        assert!(!s.busy());
    }

    #[test]
    fn a_host_keyer_confirms_before_the_text_and_after_its_tail() {
        let mut s = Sender::new(8000);
        s.host = true;
        let (t, o) = (timing(true), Options::default());
        let t0 = SystemTime::UNIX_EPOCH;
        s.send("CQ TEST", t, t0);
        assert_eq!(s.take(), [Output::Ptt(true)]);
        s.tick(t0 + Duration::from_secs(2), t, o).unwrap();
        assert!(s.take().is_empty(), "the lead is the host's");
        s.keyed(true, o).unwrap();
        let out = s.take();
        let [Output::Burst(b)] = &out[..] else {
            panic!("{out:?}")
        };
        assert_eq!(b.at, t0 + Duration::from_secs(2));
        let end = b.end();
        s.send(" QRZ", t, end - ms(1));
        s.tick(end, t, o).unwrap();
        assert!(matches!(s.take()[..], [Output::Burst(_)]));
        s.tick(end + Duration::from_secs(5), t, o).unwrap();
        assert_eq!(s.take(), [Output::Ptt(false)]);
        assert!(s.busy(), "the host is still on its tail");
        s.send("AGN", t, end + Duration::from_secs(5));
        s.keyed(false, o).unwrap();
        assert_eq!(s.take(), [Output::Ptt(true)]);
        assert!(s.abort());
        assert_eq!(s.take(), [Output::Ptt(false)]);
        s.keyed(false, o).unwrap();
        s.keyed(true, o).unwrap();
        assert!(!s.busy() && s.take().is_empty());
    }
}
