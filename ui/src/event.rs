use rtty::{Burst, Decode, Pick};

use crate::console::Bank;
use crate::heard::Heard;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Running(bool),
    Decode(Decode),
    Pick(Pick),
    Heard(Heard),
    Number(String),
    Message {
        bank: Bank,
        index: usize,
        text: String,
    },
    Burst(Burst),
    Halt,
    Ptt(bool),
    OpenWav,
    Settings,
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
