use std::time::{Duration, SystemTime};

use crate::heard::Heard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ft8,
    Ft4,
    Rtty,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Ft8 => "FT8",
            Mode::Ft4 => "FT4",
            Mode::Rtty => "RTTY",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub callsign: String,
    pub grid: Option<String>,
    pub hz: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Qso {
    pub mode: Mode,
    pub callsign: String,
    pub grid: String,
    pub sent: i32,
    pub rcvd: i32,
    pub hz: f32,
    pub at: SystemTime,
}

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
pub enum Event {
    Running(bool),
    Decode(rtty::Decode),
    Pick(Pick),
    Heard(Heard),
    Number(String),
    Message { index: usize, text: String },
    QsoCompleted(Qso),
    Burst(Burst),
    Halt,
    Ptt(bool),
    OpenWav,
}

type Handler = Box<dyn FnMut(&Event)>;

pub(crate) const EVENTS_KEPT: usize = 256;

#[derive(Default)]
pub(crate) struct Events {
    queue: Vec<Event>,
    handlers: Vec<Handler>,
}

impl Events {
    pub fn on(&mut self, handler: impl FnMut(&Event) + 'static) {
        self.handlers.push(Box::new(handler));
    }

    pub fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.queue)
    }

    pub fn emit(&mut self, event: Event) {
        for h in &mut self.handlers {
            h(&event);
        }
        if self.queue.len() >= EVENTS_KEPT {
            self.queue.remove(0);
        }
        self.queue.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_waits_with_silence_and_ends_after_its_samples() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(15500);
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
    fn handlers_see_every_event_and_the_queue_keeps_the_last() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = seen.clone();
        let mut e = Events::default();
        e.on(move |_| count.set(count.get() + 1));
        for _ in 0..EVENTS_KEPT + 4 {
            e.emit(Event::Halt);
        }
        e.emit(Event::Ptt(true));
        assert_eq!(seen.get(), EVENTS_KEPT + 5);
        let q = e.take();
        assert_eq!((q.len(), q.last()), (EVENTS_KEPT, Some(&Event::Ptt(true))));
        assert!(e.take().is_empty());
    }
}
