use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::{RichText, Ui};
use rtty::{Decode, Modem, Options, Parity};

use crate::event::{Burst, Event, Events, Mode, Pick, Qso};
use crate::sequence::{Contact, Sequencer, MESSAGES};
use crate::text::texts;
use crate::wav::Playback;

pub const MAX_DECODES: usize = 500;

const LEAD: Duration = Duration::from_millis(100);

const LATE: Duration = Duration::from_millis(1000);

const NARROW: f32 = 360.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Proto {
    #[default]
    Ft8,
    Ft4,
}

impl Proto {
    pub const ALL: [Proto; 2] = [Proto::Ft8, Proto::Ft4];

    pub fn id(self) -> &'static str {
        match self {
            Proto::Ft8 => "ft8",
            Proto::Ft4 => "ft4",
        }
    }

    pub fn mode(self) -> Mode {
        match self {
            Proto::Ft8 => Mode::Ft8,
            Proto::Ft4 => Mode::Ft4,
        }
    }

    pub fn period(self) -> Duration {
        match self {
            Proto::Ft8 => Duration::from_secs(15),
            Proto::Ft4 => Duration::from_millis(7500),
        }
    }

    pub fn open(self, rate: u32, o: Options) -> Result<Box<dyn Modem + Send>, String> {
        Ok(match self {
            Proto::Ft8 => Box::new(rtty::Ft8::new(rate, o)?),
            Proto::Ft4 => Box::new(rtty::Ft4::new(rate, o)?),
        })
    }
}

pub fn next_slot(now: SystemTime, period: Duration, parity: Parity) -> SystemTime {
    let p = period.as_nanos();
    let t = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut k = t.div_ceil(p);
    let at = |k: u128| UNIX_EPOCH + Duration::from_nanos((k * p) as u64);
    if Parity::of(at(k), period) != parity {
        k += 1;
    }
    at(k)
}

pub fn slot_start(now: SystemTime, period: Duration) -> SystemTime {
    let p = period.as_nanos();
    let t = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    UNIX_EPOCH + Duration::from_nanos((t / p * p) as u64)
}

fn other(p: Parity) -> Parity {
    match p {
        Parity::Even => Parity::Odd,
        Parity::Odd => Parity::Even,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FtTools {
    pub start: bool,
    pub mode: bool,
    pub parity: bool,
    pub enable: bool,
    pub halt: bool,
    pub wav: bool,
}

impl FtTools {
    pub const ALL: FtTools = FtTools {
        start: true,
        mode: true,
        parity: true,
        enable: true,
        halt: true,
        wav: true,
    };

    pub const NONE: FtTools = FtTools {
        start: false,
        mode: false,
        parity: false,
        enable: false,
        halt: false,
        wav: false,
    };
}

impl Default for FtTools {
    fn default() -> Self {
        FtTools::ALL
    }
}

struct Rx {
    rate: u32,
    proto: Proto,
    modem: Box<dyn Modem + Send>,
}

pub struct FtPanel {
    pub language: String,
    pub tools: FtTools,
    pub proto: Proto,
    pub my_call: String,
    pub host_call: bool,
    pub my_grid: String,
    pub rx_hz: f32,
    pub tx_hz: f32,
    pub parity: Parity,
    pub options: Options,
    pub seq: Sequencer,
    wav: Option<Playback>,
    sim_slot: Option<SystemTime>,
    decodes: Vec<Decode>,
    rx: Option<Rx>,
    tx: Option<Rx>,
    out_rate: u32,
    running: bool,
    due: Option<SystemTime>,
    tx_end: Option<SystemTime>,
    ptt: bool,
    events: Events,
    status: String,
}

impl Default for FtPanel {
    fn default() -> Self {
        FtPanel::new("en")
    }
}

impl FtPanel {
    pub fn new(language: &str) -> Self {
        FtPanel {
            language: language.into(),
            tools: FtTools::ALL,
            proto: Proto::Ft8,
            my_call: String::new(),
            host_call: false,
            my_grid: String::new(),
            rx_hz: 1500.0,
            tx_hz: 1500.0,
            parity: Parity::Even,
            options: Options::default(),
            seq: Sequencer::default(),
            wav: None,
            sim_slot: None,
            decodes: Vec::new(),
            rx: None,
            tx: None,
            out_rate: 48000,
            running: false,
            due: None,
            tx_end: None,
            ptt: false,
            events: Events::default(),
            status: String::new(),
        }
    }

    fn t(&self, key: &'static str) -> &'static str {
        texts().get(&self.language, key)
    }

    pub fn settings(&self) -> Vec<(&'static str, String)> {
        let o = &self.options;
        let odd = self.parity == Parity::Odd;
        vec![
            ("mode", self.proto.id().into()),
            ("my_call", self.my_call.clone()),
            ("my_grid", self.my_grid.clone()),
            ("rx_hz", self.rx_hz.to_string()),
            ("tx_hz", self.tx_hz.to_string()),
            ("parity", if odd { "odd" } else { "even" }.into()),
            ("auto", self.seq.auto.to_string()),
            ("min_hz", o.min_hz.to_string()),
            ("max_hz", o.max_hz.to_string()),
            ("passes", o.ft.passes.to_string()),
            ("candidates", o.ft.candidates.to_string()),
            ("rx_width", o.ft.rx_width.to_string()),
            ("blanker", o.ft.blanker.to_string()),
            ("fading", o.ft.fading.to_string()),
        ]
    }

    pub fn load_setting(&mut self, key: &str, value: &str) {
        fn set<T: std::str::FromStr>(v: &mut T, s: &str) {
            if let Ok(x) = s.trim().parse() {
                *v = x;
            }
        }
        let o = &mut self.options;
        match key {
            "mode" => {
                self.proto = if value == "ft4" {
                    Proto::Ft4
                } else {
                    Proto::Ft8
                }
            }
            "my_call" => self.my_call = value.trim().to_ascii_uppercase(),
            "my_grid" => self.my_grid = value.trim().to_ascii_uppercase(),
            "rx_hz" => set(&mut self.rx_hz, value),
            "tx_hz" => set(&mut self.tx_hz, value),
            "parity" => {
                self.parity = if value == "odd" {
                    Parity::Odd
                } else {
                    Parity::Even
                }
            }
            "auto" => set(&mut self.seq.auto, value),
            "min_hz" => set(&mut o.min_hz, value),
            "max_hz" => set(&mut o.max_hz, value),
            "passes" => set(&mut o.ft.passes, value),
            "candidates" => set(&mut o.ft.candidates, value),
            "rx_width" => set(&mut o.ft.rx_width, value),
            "blanker" => set(&mut o.ft.blanker, value),
            "fading" => set(&mut o.ft.fading, value),
            _ => {}
        }
        self.options = self.options.sanitized();
        self.rx_hz = self.rx_hz.clamp(0.0, self.options.max_hz);
        self.tx_hz = self.tx_hz.clamp(0.0, self.options.max_hz);
    }

    pub fn on_event(&mut self, handler: impl FnMut(&Event) + 'static) {
        self.events.on(handler);
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.take()
    }

    pub fn decodes(&self) -> &[Decode] {
        &self.decodes
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn sending(&self) -> bool {
        self.ptt
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_output_rate(&mut self, rate: u32) {
        self.out_rate = rate;
    }

    pub fn start(&mut self) {
        if !self.running {
            self.running = true;
            self.status.clear();
            self.events.emit(Event::Running(true));
        }
    }

    pub fn stop(&mut self) {
        self.halt();
        self.wav = None;
        if self.running {
            self.running = false;
            self.rx = None;
            self.events.emit(Event::Running(false));
        }
    }

    pub fn halt(&mut self) {
        self.seq.halt();
        self.due = None;
        if self.tx_end.take().is_some() {
            self.events.emit(Event::Halt);
        }
        self.set_ptt(false);
    }

    fn set_ptt(&mut self, on: bool) {
        if self.ptt != on {
            self.ptt = on;
            self.events.emit(Event::Ptt(on));
        }
    }

    fn sync(&mut self) {
        self.seq.my_call = self.my_call.trim().to_ascii_uppercase();
        self.seq.my_grid = self.my_grid.trim().to_ascii_uppercase();
    }

    fn options(&self) -> Options {
        Options {
            rx_hz: Some(self.rx_hz),
            ..self.options
        }
    }

    fn modem(
        slot: &mut Option<Rx>,
        rate: u32,
        proto: Proto,
        o: Options,
    ) -> Result<&mut Rx, String> {
        if !slot
            .as_ref()
            .is_some_and(|r| r.rate == rate && r.proto == proto)
        {
            *slot = Some(Rx {
                rate,
                proto,
                modem: proto.open(rate, o)?,
            });
        }
        let r = slot.as_mut().expect("opened");
        if *r.modem.options() != o {
            r.modem.set_options(o);
        }
        Ok(r)
    }

    pub fn push_samples(&mut self, rate: u32, samples: &[f32], at: SystemTime) {
        if self.wav.is_none() {
            self.feed(rate, samples, at);
        }
    }

    pub fn play_wav(&mut self, samples: Vec<f32>, rate: u32) {
        self.start();
        self.rx = None;
        self.wav = Some(Playback::new(samples, rate));
    }

    pub fn load_wav(&mut self, path: &std::path::Path) -> Result<(), String> {
        let (samples, rate) = crate::wav::read(path)?;
        self.play_wav(samples, rate);
        Ok(())
    }

    pub fn stop_wav(&mut self) {
        if self.wav.take().is_some() {
            self.rx = None;
        }
    }

    pub fn playing_wav(&self) -> bool {
        self.wav.is_some()
    }

    fn pump(&mut self, now: SystemTime) {
        let period = self.proto.period();
        let Some(w) = self.wav.as_mut() else {
            return;
        };
        let chunk = w.take(now, |t| slot_start(t, period));
        let done = w.done();
        if let Some((rate, x, at)) = chunk {
            self.feed(rate, &x, at);
        }
        if done {
            self.stop_wav();
        }
    }

    fn feed(&mut self, rate: u32, samples: &[f32], at: SystemTime) {
        if !self.running {
            return;
        }
        self.sync();
        let o = self.options();
        let decodes = match Self::modem(&mut self.rx, rate, self.proto, o) {
            Ok(r) => r.modem.push(samples, at),
            Err(e) => {
                self.status = e;
                return;
            }
        };
        for d in decodes {
            if let Some(c) = self.seq.heard(&d) {
                let q = self.qso(c, d.at);
                self.events.emit(Event::QsoCompleted(q));
            }
            self.decodes.push(d.clone());
            self.events.emit(Event::Decode(d));
        }
        if self.decodes.len() > MAX_DECODES {
            self.decodes.drain(..self.decodes.len() - MAX_DECODES);
        }
    }

    fn qso(&self, c: Contact, at: SystemTime) -> Qso {
        Qso {
            mode: self.proto.mode(),
            callsign: c.call,
            grid: c.grid,
            sent: c.sent,
            rcvd: c.rcvd,
            hz: self.tx_hz,
            at,
        }
    }

    pub fn select(&mut self, n: usize) {
        let Some(d) = self.decodes.get(n).cloned() else {
            return;
        };
        self.sync();
        self.rx_hz = d.hz;
        if let Some(s) = d.slot {
            self.parity = other(Parity::of(s, self.proto.period()));
        }
        let done = self.seq.select(&d);
        if let Some(f) = d.fields.as_ref().filter(|f| self.seq.his_call == f.de) {
            let grid = Some(self.seq.his_grid.clone()).filter(|g| !g.is_empty());
            self.events.emit(Event::Pick(Pick {
                callsign: f.de.clone(),
                grid,
                hz: d.hz,
            }));
        }
        if let Some(c) = done {
            let q = self.qso(c, d.at);
            self.events.emit(Event::QsoCompleted(q));
        }
    }

    pub fn tick(&mut self, now: SystemTime) {
        self.pump(now);
        if let Some(end) = self.tx_end {
            if now >= end {
                self.tx_end = None;
                self.seq.sent();
                self.set_ptt(false);
            }
            return;
        }
        if !self.running || !self.seq.enabled {
            self.due = None;
            return;
        }
        let period = self.proto.period();
        let due = self
            .due
            .filter(|d| Parity::of(*d, period) == self.parity && *d + LATE >= now)
            .unwrap_or_else(|| next_slot(now, period, self.parity));
        self.due = Some(due);
        if now + LEAD < due {
            return;
        }
        self.due = None;
        self.sync();
        let text = self.seq.next();
        let (rate, proto, o, hz) = (self.out_rate, self.proto, self.options(), self.tx_hz);
        let tx = Self::modem(&mut self.tx, rate, proto, o)
            .and_then(|m| m.modem.transmit(&text, hz, due));
        match tx {
            Ok(tx) => {
                let b = Burst {
                    at: tx.start,
                    rate,
                    samples: tx.audio,
                };
                self.tx_end = Some(b.end());
                self.status.clear();
                self.set_ptt(true);
                self.events.emit(Event::Burst(b));
            }
            Err(e) => {
                self.seq.halt();
                self.status = e;
            }
        }
    }

    pub fn simulate(&mut self, text: &str, now: SystemTime) {
        let text = text.trim().to_ascii_uppercase();
        if text.is_empty() {
            return;
        }
        self.start();
        let period = self.proto.period();
        let source = match self.proto {
            Proto::Ft8 => rtty::sim::Source::Ft8,
            Proto::Ft4 => rtty::sim::Source::Ft4,
        };
        let mut band = rtty::sim::Band::slot(source);
        band.rate = 12000;
        let station = rtty::sim::Station::new(&text, self.tx_hz, 0.0, -5.0);
        let slot = next_slot(now, period, Parity::of(now, period));
        let slot = self.sim_slot.filter(|s| *s >= slot).unwrap_or(slot);
        match rtty::sim::render(&band, source, &[station], 1) {
            Ok(mut audio) => {
                audio.resize((period.as_secs_f32() * band.rate as f32) as usize, 0.0);
                self.sim_slot = Some(slot + period);
                self.feed(band.rate, &audio, slot);
                self.feed(band.rate, &[0.0; 1200], slot + period);
            }
            Err(e) => self.status = e,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.toolbar(ui);
        ui.add_space(4.0);
        self.body(ui);
    }

    pub fn toolbar(&mut self, ui: &mut Ui) {
        let tools = self.tools;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if tools.start {
                let label = if self.running {
                    self.t("stop").to_string()
                } else {
                    self.t("start").to_string()
                };
                if ui
                    .add(egui::Button::new(label).selected(self.running))
                    .clicked()
                {
                    if self.running {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
            }
            if tools.mode {
                for p in Proto::ALL {
                    ui.selectable_value(&mut self.proto, p, p.mode().name());
                }
            }
            if tools.parity {
                let (even, odd) = (self.t("even"), self.t("odd"));
                let (even_tip, odd_tip) = (self.t("even.tip"), self.t("odd.tip"));
                ui.selectable_value(&mut self.parity, Parity::Even, even)
                    .on_hover_text(even_tip);
                ui.selectable_value(&mut self.parity, Parity::Odd, odd)
                    .on_hover_text(odd_tip);
            }
            if tools.enable {
                let label = self.t("enable");
                ui.toggle_value(&mut self.seq.enabled, label);
            }
            if tools.halt {
                let mut b = egui::Button::new(self.t("halt"));
                if self.ptt {
                    b = b.fill(crate::alert(ui.visuals()).gamma_multiply(0.5));
                }
                if ui.add_enabled(self.ptt, b).clicked() {
                    self.halt();
                }
            }
            if tools.wav {
                if self.wav.is_some() {
                    if ui.button(self.t("stop_wav")).clicked() {
                        self.stop_wav();
                    }
                } else if ui.button(self.t("open_wav")).clicked() {
                    self.events.emit(Event::OpenWav);
                }
            }
        });
    }

    pub fn body(&mut self, ui: &mut Ui) {
        self.tick(SystemTime::now());
        if self.running || self.wav.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !self.status.is_empty() {
            ui.colored_label(crate::alert(ui.visuals()), &self.status);
        }
        self.decodes_ui(ui);
        ui.separator();
        self.band_ui(ui);
        self.dx_ui(ui);
        self.messages_ui(ui);
    }

    fn line(&self, d: &Decode, narrow: bool) -> String {
        let secs = d
            .slot
            .and_then(|s| s.duration_since(UNIX_EPOCH).ok())
            .map(|s| s.as_secs() % 86400);
        let utc = secs.map_or("------".into(), |s| {
            format!("{:02}{:02}{:02}", s / 3600, s / 60 % 60, s % 60)
        });
        if narrow {
            format!("{:>3.0} {:>4.0} {}", d.snr, d.hz, d.text)
        } else {
            let dt = d.dt.unwrap_or(0.0);
            format!("{utc} {:>3.0} {dt:>4.1} {:>4.0} {}", d.snr, d.hz, d.text)
        }
    }

    fn decodes_ui(&mut self, ui: &mut Ui) {
        let narrow = ui.available_width() < NARROW;
        let row = ui.spacing().interact_size.y;
        let reserved = (MESSAGES as f32 + 4.0) * (row + ui.spacing().item_spacing.y);
        let height = (ui.available_height() - reserved).max(4.0 * row);
        let header = self.t(if narrow { "columns.narrow" } else { "columns" });
        ui.label(RichText::new(header).monospace().weak());
        let my_call = self.my_call.trim().to_ascii_uppercase();
        let (cq, mine) = (crate::accent(ui.visuals()), crate::alert(ui.visuals()));
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("ft-decodes")
            .max_height(height)
            .auto_shrink([false, true])
            .stick_to_bottom(true)
            .show_rows(ui, row, self.decodes.len(), |ui, range| {
                for n in range {
                    let d = &self.decodes[n];
                    let to = d.fields.as_ref().map_or("", |f| f.to.as_str());
                    let mut text = RichText::new(self.line(d, narrow)).monospace();
                    if !my_call.is_empty() && to == my_call {
                        text = text.color(mine).strong();
                    } else if to.starts_with("CQ") {
                        text = text.color(cq);
                    }
                    let b = egui::Button::selectable(false, text)
                        .truncate()
                        .min_size(egui::vec2(ui.available_width(), row));
                    if ui.add(b).clicked() {
                        picked = Some(n);
                    }
                }
            });
        if self.decodes.is_empty() {
            let key = match (self.wav.is_some(), self.running) {
                (true, _) => "decoding",
                (_, true) => "waiting",
                _ => "idle",
            };
            ui.weak(self.t(key));
        }
        if let Some(n) = picked {
            self.select(n);
        }
    }

    fn band_ui(&mut self, ui: &mut Ui) {
        let top = self.options.max_hz.max(1.0);
        let width = ui.available_width();
        let (rect, r) = ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::click());
        let x = |hz: f32| rect.left() + rect.width() * (hz / top).clamp(0.0, 1.0);
        let p = ui.painter();
        p.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        let last = self.decodes.last().and_then(|d| d.slot);
        let weak = ui.visuals().weak_text_color();
        for d in self.decodes.iter().rev().take_while(|d| d.slot == last) {
            p.vline(x(d.hz), rect.y_range(), egui::Stroke::new(1.0, weak));
        }
        let mark = |hz: f32, c: egui::Color32| {
            let r = egui::Rect::from_x_y_ranges(x(hz)..=x(hz + 50.0), rect.y_range());
            p.rect_filled(r, 0.0, c.gamma_multiply(0.6));
        };
        mark(self.rx_hz, crate::good(ui.visuals()));
        mark(self.tx_hz, crate::alert(ui.visuals()));
        let hz = |pos: egui::Pos2| ((pos.x - rect.left()) / rect.width() * top).round();
        if let Some(pos) = r.interact_pointer_pos() {
            if r.clicked() {
                self.tx_hz = hz(pos);
            } else if r.secondary_clicked() {
                self.rx_hz = hz(pos);
            }
        }
        r.on_hover_text(self.t("band.tip"));
        ui.horizontal_wrapped(|ui| {
            let (rx, tx, same) = (self.t("rx"), self.t("tx"), self.t("tx_to_rx"));
            let range = 0.0..=top;
            ui.label(rx);
            ui.add(
                egui::DragValue::new(&mut self.rx_hz)
                    .range(range.clone())
                    .suffix(" Hz"),
            );
            ui.label(tx);
            ui.add(
                egui::DragValue::new(&mut self.tx_hz)
                    .range(range)
                    .suffix(" Hz"),
            );
            if ui.button(same).clicked() {
                self.tx_hz = self.rx_hz;
            }
        });
    }

    fn dx_ui(&mut self, ui: &mut Ui) {
        let dx = self.t("dx");
        let (call_hint, grid_hint, report_tip) =
            (self.t("dx.call"), self.t("dx.grid"), self.t("report.tip"));
        ui.horizontal(|ui| {
            ui.label(dx);
            let gap = ui.spacing().item_spacing.x;
            let report = 40.0;
            let grid = 48.0;
            let call = (ui.available_width() - grid - report - 2.0 * gap).max(48.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.seq.his_call)
                    .hint_text(call_hint)
                    .desired_width(call),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.seq.his_grid)
                    .hint_text(grid_hint)
                    .char_limit(6)
                    .desired_width(grid),
            );
            ui.add_sized(
                [report, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut self.seq.report).range(-30..=30),
            )
            .on_hover_text(report_tip);
        });
        self.seq.his_call.make_ascii_uppercase();
        self.seq.his_grid.make_ascii_uppercase();
    }

    fn messages_ui(&mut self, ui: &mut Ui) {
        self.sync();
        egui::Grid::new("ft-messages")
            .num_columns(2)
            .spacing([6.0, 2.0])
            .show(ui, |ui| {
                for n in 1..=MESSAGES {
                    let text = self.seq.message(n);
                    ui.radio_value(&mut self.seq.tx, n, format!("Tx{n}"));
                    ui.add(egui::Label::new(RichText::new(text).monospace()).truncate());
                    ui.end_row();
                }
            });
    }

    pub fn settings_ui(&mut self, ui: &mut Ui) {
        let t = |k| texts().get(&self.language, k);
        if !self.host_call {
            crate::field(ui, t("my_call"), |ui| {
                ui.text_edit_singleline(&mut self.my_call)
            });
        }
        crate::field(ui, t("my_grid"), |ui| {
            ui.text_edit_singleline(&mut self.my_grid)
        });
        let o = &mut self.options;
        fn hz(v: &mut f32, lo: f32, hi: f32) -> egui::DragValue<'_> {
            egui::DragValue::new(v).range(lo..=hi).suffix(" Hz")
        }
        crate::field(ui, t("min_hz"), |ui| ui.add(hz(&mut o.min_hz, 0.0, 5000.0)));
        crate::field(ui, t("max_hz"), |ui| {
            ui.add(hz(&mut o.max_hz, 100.0, rtty::MAX_HZ))
        });
        crate::field(ui, t("rx_width"), |ui| {
            ui.add(hz(&mut o.ft.rx_width, 0.0, 1000.0))
        });
        crate::field(ui, t("passes"), |ui| {
            ui.add(egui::DragValue::new(&mut o.ft.passes).range(1..=10))
        });
        crate::field(ui, t("candidates"), |ui| {
            ui.add(egui::DragValue::new(&mut o.ft.candidates).range(1..=5000))
        });
        crate::flags(
            ui,
            &mut [
                (&mut self.seq.auto, t("auto")),
                (&mut o.ft.blanker, t("blanker")),
                (&mut o.ft.fading, t("fading")),
            ],
        );
        self.my_call = self.my_call.trim().to_ascii_uppercase();
        self.my_grid = self.my_grid.trim().to_ascii_uppercase();
        self.options = self.options.sanitized();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::tests::decode;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_793_000_000 / 30 * 30 + secs)
    }

    fn panel() -> FtPanel {
        let mut p = FtPanel::new("en");
        p.my_call = "JA1ZLO".into();
        p.my_grid = "PM95".into();
        p.set_output_rate(12000);
        p.start();
        p.take_events();
        p
    }

    #[test]
    fn slots_of_either_parity_follow_now() {
        let p = Duration::from_secs(15);
        assert_eq!(next_slot(at(1), p, Parity::Odd), at(15));
        assert_eq!(next_slot(at(1), p, Parity::Even), at(30));
        assert_eq!(next_slot(at(0), p, Parity::Even), at(0));
        let p4 = Duration::from_millis(7500);
        assert_eq!(
            next_slot(at(1), p4, Parity::Odd),
            at(0) + Duration::from_millis(7500)
        );
    }

    #[test]
    fn a_clicked_report_to_me_completes_the_qso_and_picks_the_station() {
        let mut p = panel();
        let mut d = decode("JA1ZLO JA1YWX R-07", -12.0);
        d.slot = Some(at(15));
        p.decodes.push(d);
        p.select(0);
        let e = p.take_events();
        assert!(matches!(&e[0], Event::Pick(k) if k.callsign == "JA1YWX" && k.hz == 900.0));
        let Some(Event::QsoCompleted(q)) = e.last() else {
            panic!("{e:?}")
        };
        assert_eq!(
            (q.callsign.as_str(), q.mode, q.rcvd),
            ("JA1YWX", Mode::Ft8, -7)
        );
        assert_eq!((p.parity, p.rx_hz), (Parity::Even, 900.0));
        assert_eq!(p.seq.next(), "JA1YWX JA1ZLO RR73");
    }

    #[test]
    fn a_stale_or_flipped_slot_is_not_sent_and_halt_unkeys() {
        let mut p = panel();
        p.seq.enabled = true;
        p.parity = Parity::Even;
        p.due = Some(at(0));
        p.tick(at(29));
        assert!(!p.sending(), "a slot left from long ago");
        assert_eq!(p.due, Some(at(30)));
        p.parity = Parity::Odd;
        p.tick(at(30));
        assert!(!p.sending(), "the slot of the other parity");
        p.tick(at(44) + Duration::from_millis(950));
        assert!(p.sending());
        let e = p.take_events();
        let Some(Event::Burst(b)) = e.last() else {
            panic!("{e:?}")
        };
        assert_eq!(b.at, at(45) + Duration::from_millis(500));
        assert_eq!(e[0], Event::Ptt(true));
        p.halt();
        assert_eq!(p.take_events(), [Event::Halt, Event::Ptt(false)]);
        assert!(!p.seq.enabled);
    }

    #[test]
    fn ptt_falls_after_the_burst_and_tx5_ends_the_sequence() {
        let mut p = panel();
        p.seq.enabled = true;
        p.seq.tx = 5;
        p.seq.his_call = "JA1YWX".into();
        p.tick(at(29) + Duration::from_millis(950));
        let end = p.tx_end.unwrap();
        p.tick(end - Duration::from_millis(1));
        assert!(p.sending());
        p.tick(end);
        assert!(!p.sending() && !p.seq.enabled);
        let e = p.take_events();
        assert_eq!(e.last(), Some(&Event::Ptt(false)));
    }

    #[test]
    fn settings_round_trip() {
        let mut a = FtPanel::new("en");
        a.proto = Proto::Ft4;
        a.my_grid = "PM95".into();
        a.rx_hz = 812.0;
        a.parity = Parity::Odd;
        a.seq.auto = false;
        a.options.ft.passes = 2;
        a.options.ft.fading = false;
        a.load_setting("my_call", " ja1zlo ");
        let mut b = FtPanel::new("ja");
        for (k, v) in a.settings() {
            b.load_setting(k, &v);
        }
        assert_eq!(b.settings(), a.settings());
        assert_eq!(b.my_call, "JA1ZLO");
        b.load_setting("passes", "x");
        b.load_setting("rx_hz", "99999");
        assert_eq!((b.options.ft.passes, b.rx_hz), (2, 3000.0));
    }

    #[test]
    fn nothing_is_decoded_while_stopped() {
        let mut p = FtPanel::new("en");
        p.push_samples(12000, &[0.0; 12000], at(0));
        assert!(p.rx.is_none() && p.take_events().is_empty());
        p.start();
        p.push_samples(12000, &[0.0; 12000], at(0));
        assert!(p.rx.is_some());
        p.stop();
        assert_eq!(
            p.take_events(),
            [Event::Running(true), Event::Running(false)]
        );
    }
}
