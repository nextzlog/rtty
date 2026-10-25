#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rtty::sim::{amplitude, Channel, Rng, NOISE};
use rtty::{Decode, Fields, Ft4, Ft8, Modem, Options, Parity, Rtty};

pub fn at(secs: f64) -> SystemTime {
    if secs >= 0.0 {
        UNIX_EPOCH + Duration::from_secs_f64(secs)
    } else {
        UNIX_EPOCH - Duration::from_secs_f64(-secs)
    }
}

pub fn secs(t: SystemTime) -> f64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ft8,
    Ft4,
    Rtty,
}

impl Mode {
    pub fn modem(self, rate: u32, options: Options) -> Box<dyn Modem> {
        match self {
            Mode::Ft8 => Box::new(Ft8::new(rate, options).unwrap()),
            Mode::Ft4 => Box::new(Ft4::new(rate, options).unwrap()),
            Mode::Rtty => Box::new(Rtty::new(rate, options).unwrap()),
        }
    }

    pub fn period(self) -> Option<f64> {
        match self {
            Mode::Ft8 => Some(15.0),
            Mode::Ft4 => Some(7.5),
            Mode::Rtty => None,
        }
    }
}

pub trait Operator {
    fn react(&mut self, heard: &[Decode], next: Option<Parity>) -> Option<String>;
    fn done(&self) -> bool;
    fn partner(&self) -> Option<String> {
        None
    }
    fn received(&self) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct Sent {
    pub start: SystemTime,
    pub parity: Option<Parity>,
    pub text: String,
}

pub struct Rig {
    pub call: String,
    pub mode: Mode,
    pub modem: Box<dyn Modem>,
    pub op: Box<dyn Operator>,
    pub rate: u32,
    pub block: usize,
    pub clock: f64,
    pub ppm: f64,
    pub hz: f32,
    pub snr: f32,
    pub channel: Channel,
    pub lose: Vec<usize>,
    pub listen: bool,
    pub sent: Vec<Sent>,
    pub heard: Vec<Decode>,
    pos: u64,
    pending: Vec<Decode>,
    tick: Option<i64>,
    last_heard: f64,
    last_act: f64,
    busy_from: f64,
    busy_until: f64,
}

impl Rig {
    pub fn new(call: &str, mode: Mode, op: impl Operator + 'static) -> Self {
        Rig {
            call: call.into(),
            mode,
            modem: mode.modem(12000, Options::default()),
            op: Box::new(op),
            rate: 12000,
            block: 1000,
            clock: 0.0,
            ppm: 0.0,
            hz: if mode == Mode::Rtty { 2125.0 } else { 1500.0 },
            snr: -10.0,
            channel: Channel::default(),
            lose: Vec::new(),
            listen: true,
            sent: Vec::new(),
            heard: Vec::new(),
            pos: 0,
            pending: Vec::new(),
            tick: None,
            last_heard: f64::MIN,
            last_act: -100.0,
            busy_from: f64::MIN,
            busy_until: f64::MIN,
        }
    }

    pub fn rate(mut self, rate: u32, block_ms: f64) -> Self {
        let options = *self.modem.options();
        self.rate = rate;
        self.block = (rate as f64 * block_ms / 1000.0).round() as usize;
        self.modem = self.mode.modem(rate, options);
        self
    }

    pub fn options(mut self, options: Options) -> Self {
        self.modem.set_options(options);
        self
    }

    pub fn clock(mut self, offset: f64) -> Self {
        self.clock = offset;
        self
    }

    pub fn ppm(mut self, ppm: f64) -> Self {
        self.ppm = ppm;
        self
    }

    pub fn hz(mut self, hz: f32) -> Self {
        self.hz = hz;
        self
    }

    pub fn snr(mut self, snr: f32) -> Self {
        self.snr = snr;
        self
    }

    pub fn channel(mut self, channel: Channel) -> Self {
        self.channel = channel;
        self
    }

    pub fn lose(mut self, lose: &[usize]) -> Self {
        self.lose = lose.to_vec();
        self
    }

    pub fn deaf(mut self) -> Self {
        self.listen = false;
        self
    }

    fn speed(&self) -> f64 {
        self.rate as f64 * (1.0 + self.ppm * 1e-6)
    }

    fn true_time(&self, pos: u64) -> f64 {
        pos as f64 / self.speed()
    }

    fn station_time(&self, pos: u64) -> f64 {
        self.clock + pos as f64 / self.rate as f64
    }
}

struct OnAir {
    from: usize,
    start: f64,
    speed: f64,
    samples: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub at: f64,
    pub from: String,
    pub text: String,
}

pub struct Air {
    pub base: f64,
    pub rigs: Vec<Rig>,
    pub log: Vec<Log>,
    on_air: Vec<OnAir>,
    seed: u64,
    rng: Vec<Rng>,
    pub now: f64,
}

pub const PTT: f64 = 0.3;

impl Air {
    pub fn new(base: f64, seed: u64, rigs: Vec<Rig>) -> Self {
        let rng = (0..rigs.len())
            .map(|i| Rng::new(seed.wrapping_mul(1000).wrapping_add(i as u64)))
            .collect();
        Air {
            base,
            rigs,
            log: Vec::new(),
            on_air: Vec::new(),
            seed,
            rng,
            now: 0.0,
        }
    }

    pub fn rig(&self, call: &str) -> &Rig {
        self.rigs.iter().find(|r| r.call == call).unwrap()
    }

    pub fn run(&mut self, limit: f64) -> f64 {
        while self.now < limit {
            self.now += 0.05;
            for i in 0..self.rigs.len() {
                self.advance(i);
            }
            if self.rigs.iter().all(|r| r.op.done()) {
                break;
            }
        }
        self.now
    }

    fn advance(&mut self, i: usize) {
        loop {
            let r = &self.rigs[i];
            let end = r.pos + r.block as u64;
            if r.true_time(end) > self.now {
                return;
            }
            let start = r.pos;
            if r.listen {
                let muted = r.true_time(start) < r.busy_until && r.true_time(end) > r.busy_from;
                let block = if muted {
                    vec![0.0; r.block]
                } else {
                    self.render(i, start, r.block)
                };
                let stamp = at(self.base + self.rigs[i].station_time(start));
                let got = self.rigs[i].modem.push(&block, stamp);
                let t = self.rigs[i].true_time(end);
                let r = &mut self.rigs[i];
                if !got.is_empty() {
                    r.last_heard = t;
                }
                r.heard.extend(got.iter().cloned());
                r.pending.extend(got);
            }
            self.rigs[i].pos = end;
            self.act(i);
        }
    }

    fn render(&mut self, i: usize, pos: u64, len: usize) -> Vec<f32> {
        let r = &self.rigs[i];
        let sigma = NOISE as f64 * (r.rate as f64 / 12000.0).sqrt();
        let t0 = r.true_time(pos);
        let t1 = r.true_time(pos + len as u64);
        let speed = r.speed();
        let rng = &mut self.rng[i];
        let mut out: Vec<f64> = (0..len).map(|_| sigma * rng.gauss()).collect();
        for a in &self.on_air {
            let a_end = a.start + a.samples.len() as f64 / a.speed;
            if a.from == i || a_end < t0 || a.start > t1 {
                continue;
            }
            for (k, v) in out.iter_mut().enumerate() {
                let u = (pos + k as u64) as f64 / speed;
                let p = (u - a.start) * a.speed;
                if p >= 0.0 && p < a.samples.len() as f64 {
                    *v += cubic(&a.samples, p);
                }
            }
        }
        out.into_iter().map(|v| v as f32).collect()
    }

    fn act(&mut self, i: usize) {
        let r = &mut self.rigs[i];
        let s = r.station_time(r.pos);
        let t = r.true_time(r.pos);
        let text = match r.mode.period() {
            Some(period) => {
                let k = ((s + self.base - 0.25) / period).floor() as i64;
                if r.tick.is_none() {
                    r.tick = Some(k);
                }
                if r.tick >= Some(k) {
                    return;
                }
                r.tick = Some(k);
                let slot = at((k - 1) as f64 * period);
                let heard: Vec<Decode> = std::mem::take(&mut r.pending)
                    .into_iter()
                    .filter(|d| d.slot.is_some_and(|x| (secs(x) - secs(slot)).abs() < 0.01))
                    .collect();
                let next = if k.rem_euclid(2) == 0 {
                    Parity::Even
                } else {
                    Parity::Odd
                };
                r.op.react(&heard, Some(next))
            }
            None => {
                if t < r.busy_until + 0.2 {
                    return;
                }
                if !r.pending.is_empty() && t - r.last_heard > 2.2 {
                    let heard = std::mem::take(&mut r.pending);
                    r.last_act = t;
                    r.op.react(&heard, None)
                } else if r.pending.is_empty() && t - r.last_act.max(r.busy_until) > 4.5 {
                    r.last_act = t;
                    r.op.react(&[], None)
                } else {
                    None
                }
            }
        };
        if let Some(text) = text {
            self.send(i, &text, s);
        }
    }

    fn send(&mut self, i: usize, text: &str, s: f64) {
        let r = &mut self.rigs[i];
        let tx = r
            .modem
            .transmit(text, r.hz, at(self.base + s))
            .unwrap_or_else(|e| panic!("{}: {e}", r.call));
        let index = r.sent.len();
        r.sent.push(Sent {
            start: tx.start,
            parity: tx.parity,
            text: text.into(),
        });
        let mut start = (secs(tx.start) - self.base - r.clock) / (1.0 + r.ppm * 1e-6);
        if r.mode == Mode::Rtty {
            start += PTT;
        }
        let speed = r.speed();
        r.busy_from = start;
        r.busy_until = start + tx.audio.len() as f64 / speed;
        let amp = amplitude(&tx.audio, r.snr, NOISE, 12000);
        let fade = if r.lose.contains(&index) { 0.0 } else { amp };
        let seed = self
            .seed
            .wrapping_mul(7919)
            .wrapping_add(self.on_air.len() as u64);
        let samples = r
            .channel
            .apply(&tx.audio, r.rate, seed)
            .into_iter()
            .map(|v| (v * fade) as f32)
            .collect();
        self.log.push(Log {
            at: start,
            from: r.call.clone(),
            text: text.into(),
        });
        self.on_air.push(OnAir {
            from: i,
            start,
            speed,
            samples,
        });
    }
}

fn cubic(x: &[f32], p: f64) -> f64 {
    let at = |i: isize| x[i.clamp(0, x.len() as isize - 1) as usize] as f64;
    let i = p.floor() as isize;
    let t = p - i as f64;
    let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Standard,
    Contest,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Idle,
    Called(String),
    Report(String),
    RogerReport(String),
    Roger(String),
    Final(String),
    Done(String),
    GaveUp,
}

pub struct FtOp {
    pub me: String,
    pub grid: String,
    pub style: Style,
    caller: bool,
    parity: Option<Parity>,
    state: State,
    last: Option<String>,
    repeats: usize,
    max_repeats: usize,
    received: Option<String>,
    target: Option<String>,
}

impl FtOp {
    pub fn caller(me: &str, grid: &str, style: Style, parity: Parity) -> Self {
        FtOp::new(me, grid, style, true, Some(parity))
    }

    pub fn answerer(me: &str, grid: &str, style: Style) -> Self {
        FtOp::new(me, grid, style, false, None)
    }

    fn new(me: &str, grid: &str, style: Style, caller: bool, parity: Option<Parity>) -> Self {
        FtOp {
            me: me.into(),
            grid: grid.into(),
            style,
            caller,
            parity,
            state: State::Idle,
            last: None,
            repeats: 0,
            max_repeats: 5,
            received: None,
            target: None,
        }
    }

    pub fn patience(mut self, repeats: usize) -> Self {
        self.max_repeats = repeats;
        self
    }

    pub fn to(mut self, call: &str) -> Self {
        self.target = Some(call.into());
        self
    }

    fn say(&mut self, state: State, text: String) -> Option<String> {
        self.state = state;
        self.repeats = 0;
        self.last = Some(text.clone());
        Some(text)
    }

    fn repeat(&mut self) -> Option<String> {
        self.repeats += 1;
        if self.repeats > self.max_repeats {
            self.state = if self.caller {
                State::Idle
            } else {
                State::GaveUp
            };
            self.repeats = 0;
            return if self.caller { self.cq() } else { None };
        }
        self.last.clone()
    }

    fn cq(&mut self) -> Option<String> {
        let cq = match self.style {
            Style::Standard => format!("CQ {} {}", self.me, self.grid),
            Style::Contest => format!("CQ TEST {} {}", self.me, self.grid),
        };
        self.say(State::Idle, cq)
    }
}

pub fn report(snr: f32) -> String {
    format!("{:+03}", (snr.round() as i32).clamp(-30, 32))
}

fn is_report(s: &str) -> bool {
    s.len() == 3 && (s.starts_with('+') || s.starts_with('-'))
}

fn is_grid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 4 && b[0].is_ascii_uppercase() && b[2].is_ascii_digit() && s != "RR73"
}

impl Operator for FtOp {
    fn react(&mut self, heard: &[Decode], next: Option<Parity>) -> Option<String> {
        let next = next?;
        if self.parity.is_some_and(|p| p != next) {
            return None;
        }
        let to_me: Vec<(Fields, f32)> = heard
            .iter()
            .filter_map(|d| {
                let f = d.fields.clone()?;
                (f.to == self.me && f.de != self.me).then_some((f, d.snr))
            })
            .collect();
        let from = |dx: &str| {
            to_me
                .iter()
                .find(|(f, _)| f.de == dx)
                .map(|(f, s)| (f.extra.clone(), *s))
        };
        let me = self.me.clone();
        match self.state.clone() {
            State::Idle if self.caller => {
                let best = to_me
                    .iter()
                    .filter(|(f, _)| is_grid(&f.extra) || f.extra.is_empty())
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                match (best, self.style) {
                    (Some((f, snr)), Style::Standard) => {
                        let text = format!("{} {me} {}", f.de, report(*snr));
                        self.say(State::Report(f.de.clone()), text)
                    }
                    (Some((f, _)), Style::Contest) => {
                        let text = format!("{} {me} R {}", f.de, self.grid);
                        self.say(State::Roger(f.de.clone()), text)
                    }
                    (None, _) => self.cq(),
                }
            }
            State::Idle => {
                let cq = heard.iter().find_map(|d| {
                    let f = d.fields.as_ref()?;
                    let wanted = self.target.as_ref().is_none_or(|t| *t == f.de);
                    (f.to.starts_with("CQ") && f.de != me && wanted).then(|| f.de.clone())
                })?;
                self.parity = Some(next);
                let text = format!("{cq} {me} {}", self.grid);
                self.say(State::Called(cq), text)
            }
            State::Report(dx) => match from(&dx) {
                Some((e, _)) if e.starts_with('R') && is_report(&e[1..]) => {
                    self.received = Some(e[1..].to_string());
                    self.say(State::Final(dx.clone()), format!("{dx} {me} RR73"))
                }
                _ => self.repeat(),
            },
            State::Final(dx) if self.caller => match from(&dx) {
                Some((e, _)) if e == "73" => {
                    self.state = State::Done(dx);
                    None
                }
                _ => self.repeat(),
            },
            State::Roger(dx) if self.caller => match from(&dx) {
                Some((e, _)) if e == "RRR" || e == "RR73" => {
                    self.received = Some(self.grid.clone());
                    self.state = State::Done(dx.clone());
                    self.last = Some(format!("{dx} {me} 73"));
                    self.last.clone()
                }
                _ => self.repeat(),
            },
            State::Called(dx) => match (from(&dx), self.style) {
                (Some((e, snr)), Style::Standard) if is_report(&e) => {
                    self.received = Some(e);
                    let text = format!("{dx} {me} R{}", report(snr));
                    self.say(State::RogerReport(dx), text)
                }
                (Some((e, _)), Style::Contest) if e.starts_with("R ") => {
                    self.received = Some(e[2..].to_string());
                    self.say(State::Roger(dx.clone()), format!("{dx} {me} RRR"))
                }
                _ => self.repeat(),
            },
            State::RogerReport(dx) => match from(&dx) {
                Some((e, _)) if e == "RR73" || e == "RRR" => {
                    let text = format!("{dx} {me} 73");
                    self.say(State::Done(dx), text)
                }
                _ => self.repeat(),
            },
            State::Roger(dx) => match from(&dx) {
                Some((e, _)) if e == "73" => {
                    self.state = State::Done(dx);
                    None
                }
                _ => self.repeat(),
            },
            State::Done(dx) if !self.caller => match from(&dx) {
                Some((e, _)) if e == "RR73" || e == "RRR" => self.last.clone(),
                _ => None,
            },
            _ => None,
        }
    }

    fn done(&self) -> bool {
        matches!(self.state, State::Done(_) | State::GaveUp)
    }

    fn partner(&self) -> Option<String> {
        match &self.state {
            State::Done(dx) => Some(dx.clone()),
            _ => None,
        }
    }

    fn received(&self) -> Option<String> {
        self.received.clone()
    }
}

pub struct Script {
    lines: Vec<String>,
    parity: Option<Parity>,
    next: usize,
}

impl Script {
    pub fn new(lines: &[String], parity: Option<Parity>) -> Self {
        Script {
            lines: lines.to_vec(),
            parity,
            next: 0,
        }
    }
}

impl Operator for Script {
    fn react(&mut self, _: &[Decode], next: Option<Parity>) -> Option<String> {
        if next != self.parity {
            return None;
        }
        let line = self.lines.get(self.next).cloned();
        self.next += 1;
        line
    }

    fn done(&self) -> bool {
        true
    }
}

pub struct RttyOp {
    pub me: String,
    caller: bool,
    exchange: String,
    state: State,
    last: Option<String>,
    repeats: usize,
    received: Option<String>,
}

impl RttyOp {
    pub fn caller(me: &str, exchange: &str) -> Self {
        RttyOp::new(me, exchange, true)
    }

    pub fn answerer(me: &str, exchange: &str) -> Self {
        RttyOp::new(me, exchange, false)
    }

    fn new(me: &str, exchange: &str, caller: bool) -> Self {
        RttyOp {
            me: me.into(),
            caller,
            exchange: exchange.into(),
            state: State::Idle,
            last: None,
            repeats: 0,
            received: None,
        }
    }

    fn say(&mut self, state: State, text: String) -> Option<String> {
        self.state = state;
        self.repeats = 0;
        self.last = Some(text.clone());
        Some(text)
    }

    fn repeat(&mut self) -> Option<String> {
        self.repeats += 1;
        if self.repeats > 6 {
            self.state = State::GaveUp;
            return None;
        }
        self.last.clone()
    }
}

pub fn is_call(w: &str) -> bool {
    (3..=7).contains(&w.len())
        && w.chars().all(|c| c.is_ascii_alphanumeric())
        && w.chars().any(|c| c.is_ascii_digit())
        && w.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 2
}

fn after<'a>(words: &[&'a str], key: &str) -> Option<&'a str> {
    let i = words.iter().position(|w| *w == key)?;
    words.get(i + 1).copied()
}

impl Operator for RttyOp {
    fn react(&mut self, heard: &[Decode], _: Option<Parity>) -> Option<String> {
        let words: Vec<&str> = heard.iter().map(|d| d.text.as_str()).collect();
        let has = |w: &str| words.contains(&w);
        let me = self.me.clone();
        match self.state.clone() {
            State::Idle if self.caller => {
                let dx = words.iter().find(|w| is_call(w) && **w != me);
                match dx {
                    Some(dx) if !has("CQ") => {
                        let text = format!("{dx} {}", self.exchange);
                        self.say(State::Report(dx.to_string()), text)
                    }
                    _ => self.say(State::Idle, format!("CQ TEST {me} {me}")),
                }
            }
            State::Idle => {
                if !has("CQ") {
                    return None;
                }
                let dx = words.iter().find(|w| is_call(w) && **w != me)?;
                self.say(State::Called(dx.to_string()), me.clone())
            }
            State::Report(dx) => {
                let call = words.iter().find(|w| is_call(w) && **w != me && **w != dx);
                if has("TU") && has("599") {
                    self.received = after(&words, "599").map(String::from);
                    self.say(State::Done(dx), format!("TU {me} TEST"))
                } else if let Some(call) = call.filter(|_| words.len() == 1) {
                    let text = format!("{call} {}", self.exchange);
                    self.say(State::Report(call.to_string()), text)
                } else {
                    self.repeat()
                }
            }
            State::Done(_) if self.caller && has("599") => self.last.clone(),
            State::Called(dx) => {
                if has(&me) && has("599") {
                    self.received = after(&words, "599").map(String::from);
                    self.say(State::RogerReport(dx), format!("TU {}", self.exchange))
                } else {
                    self.repeat()
                }
            }
            State::RogerReport(dx) => {
                if (has(&dx) || has("TEST")) && !has("CQ") {
                    let dx = words
                        .iter()
                        .find(|w| is_call(w))
                        .map_or(dx, |w| w.to_string());
                    self.state = State::Done(dx);
                    None
                } else {
                    self.repeat()
                }
            }
            _ => None,
        }
    }

    fn done(&self) -> bool {
        matches!(self.state, State::Done(_) | State::GaveUp)
    }

    fn partner(&self) -> Option<String> {
        match &self.state {
            State::Done(dx) => Some(dx.clone()),
            _ => None,
        }
    }

    fn received(&self) -> Option<String> {
        self.received.clone()
    }
}

pub fn texts(air: &Air, call: &str) -> Vec<String> {
    air.rig(call).sent.iter().map(|s| s.text.clone()).collect()
}

pub fn check_slots(air: &Air, from: &str, to: &str) {
    let tx = air.rig(from);
    let rx = air.rig(to);
    let period = Duration::from_secs_f64(tx.mode.period().unwrap());
    for d in rx
        .heard
        .iter()
        .filter(|d| tx.sent.iter().any(|s| s.text == d.text))
    {
        let slot = d.slot.unwrap();
        let heard = secs(slot) + 0.5 + d.dt.unwrap() as f64 - rx.clock;
        let s = tx
            .sent
            .iter()
            .filter(|s| s.text == d.text)
            .min_by(|a, b| {
                let e = |s: &&Sent| (secs(s.start) - tx.clock - heard).abs();
                e(a).total_cmp(&e(b))
            })
            .unwrap();
        assert_eq!(Some(Parity::of(slot, period)), s.parity, "{}", d.text);
        let start = secs(s.start) - tx.clock;
        assert!((start - heard).abs() < 0.2, "{}: {start} {heard}", d.text);
    }
}
