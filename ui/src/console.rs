use std::time::{Duration, SystemTime};

use egui::{Color32, RichText, Ui};
use rtty::{Modem, Options, RttyOptions, Unit};

use crate::event::{Event, Events, Pick};
use crate::heard;
use crate::sender::{Output, Sender, Timing};
use crate::terminal::{is_callsign, Terminal};
use crate::text::texts;
use crate::wav::Playback;

pub const BAUDS: [f32; 4] = [45.45, 50.0, 75.0, 100.0];

pub const SCHEMES: usize = 5;

const SCHEME_KEYS: [&str; SCHEMES] = ["scheme0", "scheme1", "scheme2", "scheme3", "scheme4"];

const NARROW: f32 = 360.0;

const CALLS_WIDTH: f32 = 100.0;

const CALLS_NARROW: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub label: String,
    pub text: String,
}

impl Message {
    pub fn new(label: &str, text: &str) -> Self {
        Message {
            label: label.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RttyTools {
    pub start: bool,
    pub messages: bool,
    pub wav: bool,
}

impl RttyTools {
    pub const ALL: RttyTools = RttyTools {
        start: true,
        messages: true,
        wav: true,
    };

    pub const NONE: RttyTools = RttyTools {
        start: false,
        messages: false,
        wav: false,
    };
}

impl Default for RttyTools {
    fn default() -> Self {
        RttyTools::ALL
    }
}

struct Rx {
    rate: u32,
    modem: rtty::Rtty,
}

pub struct RttyPanel {
    pub language: String,
    pub tools: RttyTools,
    pub my_call: String,
    pub host_call: bool,
    pub messages: Vec<Message>,
    pub host_sends: bool,
    pub options: RttyOptions,
    pub timing: Timing,
    pub scheme: usize,
    pub highlight: String,
    terminal: Terminal,
    rx: Option<Rx>,
    sender: Sender,
    typed: String,
    wav: Option<Playback>,
    sim_at: Option<SystemTime>,
    running: bool,
    level: f64,
    line: Vec<String>,
    events: Events,
    status: String,
}

impl Default for RttyPanel {
    fn default() -> Self {
        RttyPanel::new("en")
    }
}

pub fn level(snr: f32) -> f64 {
    1.0 / (1.0 + 10f64.powf(-snr as f64 / 10.0))
}

fn scheme(visuals: &egui::Visuals, n: usize) -> (Color32, Color32, Color32) {
    let rgb = Color32::from_rgb;
    let dark = rgb(0x78, 0xA0, 0xFF);
    match n {
        1 => (Color32::BLACK, Color32::WHITE, Color32::BLUE),
        2 => (rgb(0xDC, 0xDC, 0xDC), rgb(0x1E, 0x1E, 0x1E), dark),
        3 => (rgb(0x40, 0xE0, 0x40), rgb(0x00, 0x10, 0x00), dark),
        4 => (rgb(0xFF, 0xB0, 0x30), rgb(0x00, 0x0B, 0x16), dark),
        _ => (
            visuals.text_color(),
            visuals.extreme_bg_color,
            crate::accent(visuals),
        ),
    }
}

impl RttyPanel {
    pub fn new(language: &str) -> Self {
        RttyPanel {
            language: language.into(),
            tools: RttyTools::ALL,
            my_call: String::new(),
            host_call: false,
            messages: Vec::new(),
            host_sends: false,
            options: RttyOptions::default(),
            timing: Timing {
                ptt: true,
                before: Duration::from_millis(300),
                after: Duration::from_millis(300),
            },
            scheme: 0,
            highlight: String::new(),
            terminal: Terminal::default(),
            rx: None,
            sender: Sender::new(48000),
            typed: String::new(),
            wav: None,
            sim_at: None,
            running: false,
            level: 0.0,
            line: Vec::new(),
            events: Events::default(),
            status: String::new(),
        }
    }

    fn t(&self, key: &'static str) -> &'static str {
        texts().get(&self.language, key)
    }

    pub fn settings(&self) -> Vec<(&'static str, String)> {
        let o = &self.options;
        let t = &self.timing;
        vec![
            ("my_call", self.my_call.clone()),
            ("mark", o.mark.to_string()),
            ("shift", o.shift.to_string()),
            ("baud", o.baud.to_string()),
            ("reverse", o.reverse.to_string()),
            ("uos", o.uos.to_string()),
            ("squelch", o.squelch.to_string()),
            ("atc", o.atc.to_string()),
            ("blanker", o.blanker.to_string()),
            ("ptt", t.ptt.to_string()),
            ("ptt_before", t.before.as_millis().to_string()),
            ("ptt_after", t.after.as_millis().to_string()),
            ("scheme", self.scheme.to_string()),
            ("highlight", self.highlight.clone()),
        ]
    }

    pub fn load_setting(&mut self, key: &str, value: &str) {
        fn set<T: std::str::FromStr>(v: &mut T, s: &str) {
            if let Ok(x) = s.trim().parse() {
                *v = x;
            }
        }
        let ms = |s: &str, d: Duration| {
            s.trim()
                .parse::<u64>()
                .map_or(d, |v| Duration::from_millis(v.min(2000)))
        };
        let o = &mut self.options;
        match key {
            "my_call" => self.my_call = value.trim().to_ascii_uppercase(),
            "mark" => set(&mut o.mark, value),
            "shift" => set(&mut o.shift, value),
            "baud" => set(&mut o.baud, value),
            "reverse" => set(&mut o.reverse, value),
            "uos" => set(&mut o.uos, value),
            "squelch" => set(&mut o.squelch, value),
            "atc" => set(&mut o.atc, value),
            "blanker" => set(&mut o.blanker, value),
            "ptt" => set(&mut self.timing.ptt, value),
            "ptt_before" => self.timing.before = ms(value, self.timing.before),
            "ptt_after" => self.timing.after = ms(value, self.timing.after),
            "scheme" => set(&mut self.scheme, value),
            "highlight" => self.highlight = value.into(),
            _ => {}
        }
        self.options = self.full_options().sanitized().rtty;
        self.scheme = self.scheme.min(SCHEMES - 1);
    }

    fn full_options(&self) -> Options {
        Options {
            rtty: self.options,
            ..Options::default()
        }
    }

    pub fn on_event(&mut self, handler: impl FnMut(&Event) + 'static) {
        self.events.on(handler);
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.take()
    }

    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn sending(&self) -> bool {
        self.sender.busy()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_output_rate(&mut self, rate: u32) {
        self.sender.rate = rate;
    }

    pub fn start(&mut self) {
        if !self.running {
            self.running = true;
            self.status.clear();
            self.events.emit(Event::Running(true));
        }
    }

    pub fn stop(&mut self) {
        if self.running {
            self.running = false;
            self.rx = None;
            self.wav = None;
            self.level = 0.0;
            self.events.emit(Event::Running(false));
        }
    }

    pub fn clear(&mut self) {
        self.terminal.clear_text();
        self.terminal.calls.clear();
    }

    fn highlighted(&self, word: &str) -> bool {
        self.highlight
            .split_whitespace()
            .any(|w| w.eq_ignore_ascii_case(word))
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
        let Some(w) = self.wav.as_mut() else {
            return;
        };
        let chunk = w.take(now, |t| t);
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
        let o = self.full_options();
        if !self.rx.as_ref().is_some_and(|r| r.rate == rate) {
            match rtty::Rtty::new(rate, o) {
                Ok(modem) => self.rx = Some(Rx { rate, modem }),
                Err(e) => {
                    self.status = e;
                    return;
                }
            }
        }
        let r = self.rx.as_mut().expect("opened");
        if *r.modem.options() != o {
            r.modem.set_options(o);
        }
        let decodes = r.modem.push(samples, at);
        if let Some(d) = decodes.last() {
            self.level = level(d.snr);
        }
        let listening = !self.sender.busy();
        let my_call = self.my_call.trim().to_ascii_uppercase();
        for d in decodes {
            let word = d.unit != Unit::LineBreak;
            for c in d.text.chars().chain(word.then_some(' ')) {
                if let Some(n) = self.terminal.push(c, &my_call, listening) {
                    self.events.emit(Event::Number(n));
                }
            }
            if word && listening {
                self.line
                    .extend(d.text.split_whitespace().map(String::from));
            } else if !word {
                let h = heard::parse(&std::mem::take(&mut self.line), &my_call);
                if h.callsign.is_some() || !h.exchange.is_empty() {
                    self.events.emit(Event::Heard(h));
                }
            }
            self.events.emit(Event::Decode(d));
        }
    }

    pub fn send(&mut self, text: &str) {
        let text = text.trim().to_ascii_uppercase();
        if text.is_empty() {
            return;
        }
        let now = SystemTime::now();
        self.sender
            .send(&format!("\r\n{text}\r\n"), self.timing, now);
        self.flush();
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn send_typed(&mut self) {
        let text = std::mem::take(&mut self.typed);
        self.send(&text);
    }

    pub fn play(&mut self, n: usize) {
        let Some(m) = self.messages.get(n) else {
            return;
        };
        let text = m.text.clone();
        self.events.emit(Event::Message {
            index: n,
            text: text.clone(),
        });
        if !self.host_sends {
            self.send(&text);
        }
    }

    pub fn halt(&mut self) {
        if self.sender.abort() {
            self.events.emit(Event::Halt);
        }
        self.flush();
    }

    fn pick(&mut self, callsign: &str) {
        self.events.emit(Event::Pick(Pick {
            callsign: callsign.into(),
            grid: None,
            hz: self.options.mark,
        }));
    }

    fn flush(&mut self) {
        for o in self.sender.take() {
            self.events.emit(match o {
                Output::Burst(b) => Event::Burst(b),
                Output::Ptt(on) => Event::Ptt(on),
            });
        }
    }

    pub fn tick(&mut self, now: SystemTime) {
        self.pump(now);
        let o = self.full_options();
        if let Err(e) = self.sender.tick(now, self.timing, o) {
            self.status = e;
        }
        self.flush();
    }

    pub fn simulate(&mut self, text: &str, now: SystemTime) {
        let text = text.trim().to_ascii_uppercase();
        if text.is_empty() {
            return;
        }
        self.start();
        let rate = 12000;
        let seconds = 1.0 + (text.len() as f32 + 4.0) * 7.5 / self.options.baud;
        let band = rtty::sim::Band::audio(rate, seconds);
        let station =
            rtty::sim::Station::new(&format!("\r\n{text}\r\n"), self.options.mark, 0.0, 15.0);
        let source = rtty::sim::Source::Rtty(self.options);
        match rtty::sim::render(&band, source, &[station], 1) {
            Ok(audio) => {
                let at = self.sim_at.filter(|t| *t > now).unwrap_or(now);
                self.sim_at = Some(at + Duration::from_secs_f32(seconds));
                self.feed(rate, &audio, at);
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
        let mut play = None;
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
            if tools.messages {
                for (n, m) in self.messages.iter().enumerate() {
                    if m.label.trim().is_empty() {
                        continue;
                    }
                    if ui.button(&m.label).on_hover_text(&m.text).clicked() {
                        play = Some(n);
                    }
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
        if let Some(n) = play {
            self.play(n);
        }
    }

    pub fn body(&mut self, ui: &mut Ui) {
        self.tick(SystemTime::now());
        if self.running || self.sender.busy() || self.wav.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if !self.status.is_empty() {
            ui.colored_label(crate::alert(ui.visuals()), &self.status);
        }
        if self.terminal.all_lines().all(|l| l.trim().is_empty()) {
            let key = match (self.wav.is_some(), self.running) {
                (true, _) => "decoding",
                (_, true) => "waiting",
                _ => "idle",
            };
            ui.weak(self.t(key));
        }
        let narrow = ui.available_width() < NARROW;
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let reserve = if narrow { 2.5 } else { 1.5 } * row;
        let height = (ui.available_height() - reserve).max(row * 3.0);
        if narrow {
            ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                self.text_ui(ui)
            });
            self.calls_row(ui);
        } else {
            ui.horizontal(|ui| {
                let gap = ui.spacing().item_spacing.x;
                let width = ui.available_width() - CALLS_WIDTH - gap;
                let down = egui::Layout::top_down(egui::Align::Min);
                ui.allocate_ui_with_layout(egui::vec2(width, height), down, |ui| self.text_ui(ui));
                ui.allocate_ui_with_layout(egui::vec2(CALLS_WIDTH, height), down, |ui| {
                    self.calls_ui(ui)
                });
            });
        }
        self.send_ui(ui);
    }

    fn text_ui(&mut self, ui: &mut Ui) {
        let (fg, bg, accent) = scheme(ui.visuals(), self.scheme);
        let mut picked = None;
        let level = self.running.then_some(self.level);
        egui::Frame::new()
            .fill(bg)
            .inner_margin(4.0)
            .show(ui, |ui| {
                if let Some(l) = level {
                    let w = ui.available_width();
                    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 3.0), egui::Sense::hover());
                    let lit = egui::Rect::from_min_size(r.min, egui::vec2(w * l as f32, 3.0));
                    ui.painter()
                        .rect_filled(lit, 1.0, crate::good(ui.visuals()));
                }
                let row = ui.text_style_height(&egui::TextStyle::Monospace);
                let lines: Vec<&str> = self.terminal.all_lines().collect();
                egui::ScrollArea::vertical()
                    .id_salt("rtty-text")
                    .auto_shrink(false)
                    .stick_to_bottom(true)
                    .show_rows(ui, row, lines.len(), |ui, range| {
                        for line in &lines[range] {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (i, word) in line.split(' ').enumerate() {
                                    if i > 0 {
                                        ui.label(RichText::new(" ").monospace());
                                    }
                                    let text = RichText::new(word).monospace();
                                    if is_callsign(word) {
                                        let text = text.color(accent).underline();
                                        let l = egui::Label::new(text).sense(egui::Sense::click());
                                        if ui.add(l).clicked() {
                                            picked = Some(word.to_string());
                                        }
                                    } else if self.highlighted(word) {
                                        ui.label(text.strong().color(accent));
                                    } else {
                                        ui.label(text.color(fg));
                                    }
                                }
                            });
                        }
                    });
            });
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    fn calls_ui(&mut self, ui: &mut Ui) {
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("rtty-calls")
            .auto_shrink(false)
            .show(ui, |ui| {
                for c in &self.terminal.calls {
                    let b = egui::Button::selectable(false, RichText::new(c).monospace())
                        .truncate()
                        .min_size(egui::vec2(ui.available_width(), 0.0));
                    if ui.add(b).clicked() {
                        picked = Some(c.clone());
                    }
                }
            });
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    fn calls_row(&mut self, ui: &mut Ui) {
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            for c in self.terminal.calls.iter().take(CALLS_NARROW) {
                if ui
                    .add(egui::Button::selectable(
                        false,
                        RichText::new(c).monospace(),
                    ))
                    .clicked()
                {
                    picked = Some(c.clone());
                }
            }
        });
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    fn send_ui(&mut self, ui: &mut Ui) {
        let (hint, send, halt) = (self.t("text.hint"), self.t("send"), self.t("halt"));
        let busy = self.sender.busy();
        let clicked = crate::text_row(ui, &mut self.typed, hint, &[send, halt]);
        if clicked.contains(&send) {
            let text = std::mem::take(&mut self.typed);
            self.send(&text);
        }
        if clicked.contains(&halt) && busy {
            self.halt();
        }
    }

    pub fn settings_ui(&mut self, ui: &mut Ui) {
        let t = |k| texts().get(&self.language, k);
        let o = &mut self.options;
        if !self.host_call {
            crate::field(ui, t("my_call"), |ui| {
                ui.text_edit_singleline(&mut self.my_call)
            });
        }
        crate::field(ui, t("mark"), |ui| {
            ui.add(
                egui::DragValue::new(&mut o.mark)
                    .range(300.0..=3000.0)
                    .suffix(" Hz"),
            )
        });
        crate::field(ui, t("shift"), |ui| {
            ui.add(
                egui::DragValue::new(&mut o.shift)
                    .range(50.0..=1000.0)
                    .suffix(" Hz"),
            )
        });
        crate::field(ui, t("baud"), |ui| {
            let w = ui.available_width();
            egui::ComboBox::from_id_salt("rtty-baud")
                .selected_text(o.baud.to_string())
                .width(w)
                .show_ui(ui, |ui| {
                    for b in BAUDS {
                        ui.selectable_value(&mut o.baud, b, b.to_string());
                    }
                })
                .response
        });
        crate::field(ui, t("squelch"), |ui| {
            ui.add(egui::Slider::new(&mut o.squelch, 0.0..=0.99))
        });
        crate::flags(
            ui,
            &mut [
                (&mut o.reverse, t("reverse")),
                (&mut o.uos, t("uos")),
                (&mut o.atc, t("atc")),
                (&mut o.blanker, t("blanker")),
                (&mut self.timing.ptt, t("ptt")),
            ],
        );
        let ms = |d: &mut Duration, ui: &mut Ui| {
            let mut v = d.as_millis() as u64;
            let r = ui.add(egui::DragValue::new(&mut v).range(0..=2000).suffix(" ms"));
            *d = Duration::from_millis(v);
            r
        };
        crate::field(ui, t("ptt_before"), |ui| ms(&mut self.timing.before, ui));
        crate::field(ui, t("ptt_after"), |ui| ms(&mut self.timing.after, ui));
        crate::field(ui, t("colours"), |ui| {
            let w = ui.available_width();
            let names: Vec<&str> = (0..SCHEMES)
                .map(|n| texts().get(&self.language, SCHEME_KEYS[n]))
                .collect();
            egui::ComboBox::from_id_salt("rtty-scheme")
                .selected_text(names[self.scheme.min(SCHEMES - 1)])
                .width(w)
                .show_ui(ui, |ui| {
                    for (n, name) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.scheme, n, *name);
                    }
                })
                .response
        });
        crate::field(ui, t("highlight"), |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.highlight).hint_text(t("highlight.hint")))
        });
        self.my_call = self.my_call.trim().to_ascii_uppercase();
    }
}
