use std::time::{Duration, SystemTime};

use egui::{Color32, RichText, Ui};
use rtty::{Command, Decode, Engine, Notice, Options, Pick, RttyOptions, Timing, Unit};

use crate::event::{Event, Events};
use crate::heard;
use crate::icon;
use crate::terminal::{is_callsign, Terminal};
use crate::text::texts;
use crate::wav::Playback;

pub const BAUDS: [f32; 4] = [45.45, 50.0, 75.0, 100.0];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bank {
    #[default]
    Cq,
    Sp,
}

impl Bank {
    pub fn id(self) -> &'static str {
        match self {
            Bank::Cq => "cq",
            Bank::Sp => "sp",
        }
    }
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
    pub bank: bool,
    pub messages: bool,
    pub wav: bool,
    pub settings: bool,
}

impl RttyTools {
    pub const ALL: RttyTools = RttyTools {
        start: true,
        bank: true,
        messages: true,
        wav: true,
        settings: true,
    };

    pub const NONE: RttyTools = RttyTools {
        start: false,
        bank: false,
        messages: false,
        wav: false,
        settings: false,
    };
}

impl Default for RttyTools {
    fn default() -> Self {
        RttyTools::ALL
    }
}

pub struct RttyPanel {
    pub language: String,
    pub tools: RttyTools,
    pub my_call: String,
    pub host_call: bool,
    pub host_ptt: bool,
    pub messages: Vec<Message>,
    pub sp_messages: Vec<Message>,
    pub bank: Bank,
    pub host_sends: bool,
    pub options: RttyOptions,
    pub timing: Timing,
    pub highlight: String,
    engine: Option<Engine>,
    outbox: Vec<Command>,
    told: (RttyOptions, Timing),
    terminal: Terminal,
    typed: String,
    wav: Option<Playback>,
    sim_at: Option<SystemTime>,
    running: bool,
    busy: bool,
    level: f64,
    line: Vec<String>,
    events: Events,
    status: String,
    dial: Option<u64>,
}

impl Default for RttyPanel {
    fn default() -> Self {
        RttyPanel::new("en")
    }
}

pub fn level(snr: f32) -> f64 {
    1.0 / (1.0 + 10f64.powf(-snr as f64 / 10.0))
}

impl RttyPanel {
    pub fn new(language: &str) -> Self {
        RttyPanel {
            language: language.into(),
            tools: RttyTools::ALL,
            my_call: String::new(),
            host_call: false,
            host_ptt: false,
            messages: Vec::new(),
            sp_messages: Vec::new(),
            bank: Bank::Cq,
            host_sends: false,
            options: RttyOptions::default(),
            timing: Timing::default(),
            highlight: String::new(),
            engine: Some(Engine::new()),
            outbox: Vec::new(),
            told: (RttyOptions::default(), Timing::default()),
            terminal: Terminal::default(),
            typed: String::new(),
            wav: None,
            sim_at: None,
            running: false,
            busy: false,
            level: 0.0,
            line: Vec::new(),
            events: Events::default(),
            status: String::new(),
            dial: None,
        }
    }

    pub fn remote(language: &str) -> Self {
        RttyPanel {
            engine: None,
            ..RttyPanel::new(language)
        }
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        self.sync();
        std::mem::take(&mut self.outbox)
    }

    pub fn notify(&mut self, n: Notice) {
        match n {
            Notice::Running(on) => {
                self.running = on;
                if !on {
                    self.level = 0.0;
                }
                self.events.emit(Event::Running(on));
            }
            Notice::Decode(d) => self.decoded(d),
            Notice::Busy(on) => self.busy = on,
            Notice::Burst(b) => self.events.emit(Event::Burst(b)),
            Notice::Halt => self.events.emit(Event::Halt),
            Notice::Ptt(on) => self.events.emit(Event::Ptt(on)),
            Notice::Dial(hz) => self.dial = Some(hz),
            Notice::Error(e) => self.status = e,
        }
    }

    fn sync(&mut self) {
        if self.options != self.told.0 {
            self.told.0 = self.options;
            self.dispatch(Command::Options(self.options));
        }
        if self.timing != self.told.1 {
            self.told.1 = self.timing;
            self.dispatch(Command::Timing(self.timing));
        }
    }

    fn dispatch(&mut self, c: Command) {
        let Some(engine) = self.engine.as_mut() else {
            self.outbox.push(c);
            return;
        };
        engine.handle(c);
        for n in engine.take() {
            self.notify(n);
        }
    }

    fn command(&mut self, c: Command) {
        self.sync();
        self.dispatch(c);
    }

    pub fn bank_messages(&self) -> &[Message] {
        match self.bank {
            Bank::Cq => &self.messages,
            Bank::Sp => &self.sp_messages,
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
            ("highlight", self.highlight.clone()),
            ("bank", self.bank.id().into()),
        ]
        .into_iter()
        .filter(|(k, _)| !(self.host_ptt && k.starts_with("ptt_")))
        .collect()
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
            "highlight" => self.highlight = value.into(),
            "bank" => self.bank = if value == "sp" { Bank::Sp } else { Bank::Cq },
            _ => {}
        }
        self.options = self.full_options().sanitized().rtty;
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
        self.busy
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_output_rate(&mut self, rate: u32) {
        self.command(Command::OutputRate(rate));
    }

    pub fn start(&mut self) {
        if !self.running {
            self.status.clear();
        }
        self.command(Command::Start);
    }

    pub fn stop(&mut self) {
        self.wav = None;
        self.command(Command::Stop);
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
        self.wav = Some(Playback::new(samples, rate));
    }

    pub fn load_wav(&mut self, path: &std::path::Path) -> Result<(), String> {
        let (samples, rate) = crate::wav::read(path)?;
        self.play_wav(samples, rate);
        Ok(())
    }

    pub fn stop_wav(&mut self) {
        self.wav = None;
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
        if self.running || self.engine.is_none() {
            let samples = samples.to_vec();
            self.command(Command::Samples { rate, samples, at });
        }
    }

    fn decoded(&mut self, d: Decode) {
        self.level = level(d.snr);
        let listening = !self.busy;
        let my_call = self.my_call.trim().to_ascii_uppercase();
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
            let mut h = heard::parse(&std::mem::take(&mut self.line), &my_call);
            h.dial_hz = self.dial;
            if h.callsign.is_some() || !h.exchange.is_empty() {
                self.events.emit(Event::Heard(h));
            }
        }
        self.events.emit(Event::Decode(d));
    }

    pub fn send(&mut self, text: &str) {
        let text = text.trim().to_ascii_uppercase();
        if text.is_empty() {
            return;
        }
        self.command(Command::Send {
            text: format!("\r\n{text}\r\n"),
            at: SystemTime::now(),
        });
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn send_typed(&mut self) {
        let text = std::mem::take(&mut self.typed);
        self.send(&text);
    }

    pub fn play(&mut self, n: usize) {
        let Some(m) = self.bank_messages().get(n) else {
            return;
        };
        let text = m.text.clone();
        self.events.emit(Event::Message {
            bank: self.bank,
            index: n,
            text: text.clone(),
        });
        if !self.host_sends {
            self.send(&text);
        }
    }

    pub fn set_ptt_lead(&mut self, lead: Option<Duration>) {
        self.command(Command::PttLead(lead));
    }

    pub fn keyed(&mut self, on: bool) {
        self.command(Command::Keyed(on));
    }

    pub fn key_failed(&mut self, reason: &str) {
        self.command(Command::KeyFailed(reason.into()));
    }

    pub fn set_dial(&mut self, hz: u64) {
        self.command(Command::Dial(hz));
    }

    pub fn halt(&mut self) {
        self.command(Command::Halt);
    }

    fn pick(&mut self, callsign: &str) {
        self.events.emit(Event::Pick(Pick {
            callsign: callsign.into(),
            hz: self.options.mark,
            dial_hz: self.dial,
        }));
    }

    pub fn tick(&mut self, now: SystemTime) {
        self.pump(now);
        self.command(Command::Tick(now));
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
        self.toolbar_ui(ui, |_| {});
    }

    pub fn toolbar_ui(&mut self, ui: &mut Ui, mode: impl FnOnce(&mut Ui)) {
        let settings = self.tools.settings;
        let label = self.t("settings");
        let mut open = false;
        crate::bar(
            ui,
            |ui| open = settings && icon::button(ui, icon::GEAR, label, false).clicked(),
            |ui| self.tools_ui(ui, mode),
        );
        if open {
            self.events.emit(Event::Settings);
        }
    }

    pub fn tools_ui(&mut self, ui: &mut Ui, mode: impl FnOnce(&mut Ui)) {
        let tools = self.tools;
        let mut play = None;
        ui.spacing_mut().item_spacing.x = 6.0;
        if tools.start {
            let (g, label) = if self.running {
                (icon::STOP, self.t("stop"))
            } else {
                (icon::PLAY, self.t("start"))
            };
            if icon::button(ui, g, label, self.running).clicked() {
                if self.running {
                    self.stop();
                } else {
                    self.start();
                }
            }
        }
        if tools.wav {
            if self.wav.is_some() {
                if icon::button(ui, icon::STOP, self.t("stop_wav"), false).clicked() {
                    self.stop_wav();
                }
            } else if icon::button(ui, icon::FOLDER_OPEN, self.t("open_wav"), false).clicked() {
                self.events.emit(Event::OpenWav);
            }
        }
        mode(ui);
        if tools.bank {
            let (name, label) = match self.bank {
                Bank::Cq => (self.t("bank.cq"), self.t("bank.cq.label")),
                Bank::Sp => (self.t("bank.sp"), self.t("bank.sp.label")),
            };
            icon::install(ui.ctx());
            let text = (icon::glyph(icon::ARROWS_LEFT_RIGHT), RichText::new(name));
            let r = ui.add(egui::Button::new(text));
            let r = icon::labelled(r, label, false).on_hover_text(self.t("bank.tip"));
            if r.clicked() {
                self.bank = match self.bank {
                    Bank::Cq => Bank::Sp,
                    Bank::Sp => Bank::Cq,
                };
            }
        }
        if tools.messages {
            for (n, m) in self.bank_messages().iter().enumerate() {
                if m.label.trim().is_empty() {
                    continue;
                }
                if ui.button(&m.label).on_hover_text(&m.text).clicked() {
                    play = Some(n);
                }
            }
        }
        if let Some(n) = play {
            self.play(n);
        }
    }

    pub fn body(&mut self, ui: &mut Ui) {
        self.tick(SystemTime::now());
        if self.running || self.busy || self.wav.is_some() {
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
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let height = (ui.available_height() - 2.0 * row - 4.0).max(row * 3.0);
        let down = egui::Layout::top_down_justified(egui::Align::Min);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), down, |ui| {
            self.text_ui(ui)
        });
        self.calls_row(ui);
        self.send_ui(ui);
    }

    fn text_ui(&mut self, ui: &mut Ui) {
        let (fg, accent) = (ui.visuals().text_color(), crate::accent(ui.visuals()));
        let mut picked = None;
        let level = self.running.then_some(self.level);
        let label = self.t("terminal");
        let frame = egui::Frame::group(ui.style())
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
        frame
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, label));
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    fn calls_row(&mut self, ui: &mut Ui) {
        let mut picked = None;
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            size,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let pad = 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
                for c in &self.terminal.calls {
                    let font = egui::TextStyle::Monospace.resolve(ui.style());
                    let w = ui
                        .painter()
                        .layout_no_wrap(c.clone(), font, Color32::WHITE)
                        .size()
                        .x;
                    if w + pad > ui.available_width() {
                        break;
                    }
                    let b = egui::Button::selectable(false, RichText::new(c).monospace());
                    if ui.add(b).clicked() {
                        picked = Some(c.clone());
                    }
                }
            },
        );
        if let Some(c) = picked {
            self.pick(&c);
        }
    }

    fn send_ui(&mut self, ui: &mut Ui) {
        let (hint, send, halt) = (self.t("text.hint"), self.t("send"), self.t("halt"));
        let busy = self.busy;
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
        if !self.host_ptt {
            crate::field(ui, t("ptt_before"), |ui| ms(&mut self.timing.before, ui));
            crate::field(ui, t("ptt_after"), |ui| ms(&mut self.timing.after, ui));
        }
        crate::field(ui, t("highlight"), |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.highlight).hint_text(t("highlight.hint")))
        });
        self.my_call = self.my_call.trim().to_ascii_uppercase();
        self.options = self.full_options().sanitized().rtty;
        self.sync();
    }
}
