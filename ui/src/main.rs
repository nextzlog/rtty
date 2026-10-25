#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::SystemTime;

use rtty_ui::audio::{input_devices, output_devices, Input, Output};
use rtty_ui::text::texts;
use rtty_ui::{Event, FtPanel, FtTools, Message, RttyPanel};

const MESSAGES: [(&str, &str); 6] = [
    ("CQ", "CQ CQ DE $M $M K"),
    ("Call", "$C DE $M K"),
    ("599", "$C 599 599 BK"),
    ("TU", "$C TU 73 DE $M SK"),
    ("QRZ", "QRZ? DE $M K"),
    ("AGN", "AGN AGN"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Ft8,
    Ft4,
    Rtty,
}

#[derive(Clone, PartialEq, Eq)]
enum Source {
    Default,
    Device(String),
}

impl Source {
    fn key(&self) -> String {
        match self {
            Source::Default => String::new(),
            Source::Device(d) => format!("device:{d}"),
        }
    }

    fn from_key(k: &str) -> Source {
        k.strip_prefix("device:")
            .map_or(Source::Default, |d| Source::Device(d.into()))
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 560.0])
            .with_min_inner_size([280.0, 320.0])
            .with_title("RTTY"),
        ..Default::default()
    };
    eframe::run_native(
        "RTTY",
        options,
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            Ok(Box::new(App::new(cc)))
        }),
    )
}

struct App {
    language: String,
    dark: bool,
    tab: Tab,
    ft: FtPanel,
    rtty: RttyPanel,
    source: Source,
    output: Option<String>,
    vox: bool,
    input_io: Option<Input>,
    output_io: Option<Output>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    settings_open: bool,
    last_call: String,
    error: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let get = |k: &str| cc.storage.and_then(|s| s.get_string(k));
        let language = get("app.language").unwrap_or_else(system_language);
        let mut ft = FtPanel::new(&language);
        let mut rtty = RttyPanel::new(&language);
        for (k, _) in ft.settings() {
            if let Some(v) = get(&format!("ft.{k}")) {
                ft.load_setting(k, &v);
            }
        }
        for (k, _) in rtty.settings() {
            if let Some(v) = get(&format!("rtty.{k}")) {
                rtty.load_setting(k, &v);
            }
        }
        rtty.messages = MESSAGES
            .iter()
            .enumerate()
            .map(|(n, (l, t))| {
                let label = get(&format!("message{n}.label")).unwrap_or(l.to_string());
                let text = get(&format!("message{n}.text")).unwrap_or(t.to_string());
                Message::new(&label, &text)
            })
            .collect();
        rtty.host_sends = true;
        App {
            language,
            dark: get("app.theme").is_none_or(|t| t != "light"),
            tab: match get("app.mode").or_else(|| get("app.tab")).as_deref() {
                Some("rtty") => Tab::Rtty,
                Some("ft4") => Tab::Ft4,
                _ => Tab::Ft8,
            },
            ft,
            rtty,
            source: Source::from_key(&get("app.input").unwrap_or_default()),
            output: get("app.output").filter(|s| !s.is_empty()),
            vox: get("app.transmit").as_deref() == Some("vox"),
            input_io: None,
            output_io: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            settings_open: false,
            last_call: String::new(),
            error: String::new(),
        }
    }

    fn t(&self, k: &'static str) -> &'static str {
        texts().get(&self.language, k)
    }

    fn run_audio(&mut self) {
        match self.tab {
            Tab::Ft8 => self.ft.proto = rtty_ui::Proto::Ft8,
            Tab::Ft4 => self.ft.proto = rtty_ui::Proto::Ft4,
            Tab::Rtty => {}
        }
        let tx = FtTools {
            mode: false,
            enable: self.vox,
            halt: self.vox,
            ..FtTools::ALL
        };
        self.ft.tools = tx;
        let wav = self.ft.playing_wav() || self.rtty.playing_wav();
        let wanted = !wav && (self.ft.running() || self.rtty.running());
        if wanted && self.input_io.is_none() {
            let name = match &self.source {
                Source::Device(d) => Some(d.as_str()),
                _ => None,
            };
            match Input::open(name) {
                Ok(i) => self.input_io = Some(i),
                Err(e) => {
                    self.error = e;
                    self.ft.stop();
                    self.rtty.stop();
                }
            }
        }
        if !wanted {
            self.input_io = None;
        }
        if self.vox && self.output_io.is_none() {
            match Output::open(self.output.as_deref()) {
                Ok(o) => {
                    self.ft.set_output_rate(o.rate);
                    self.rtty.set_output_rate(o.rate);
                    self.output_io = Some(o);
                }
                Err(e) => {
                    self.error = e;
                    self.vox = false;
                }
            }
        }
        if !self.vox {
            self.output_io = None;
        }
        if let Some((x, at)) = self.input_io.as_mut().and_then(|i| i.take()) {
            let rate = self.input_io.as_ref().map_or(48000, |i| i.rate);
            self.ft.push_samples(rate, &x, at);
            self.rtty.push_samples(rate, &x, at);
        }
    }

    fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::Burst(b) => {
                    if let Some(o) = &self.output_io {
                        o.play(&b.lead_in(SystemTime::now()));
                    }
                }
                Event::Halt => {
                    if let Some(o) = &self.output_io {
                        o.stop();
                    }
                }
                Event::Pick(p) => self.last_call = p.callsign,
                Event::OpenWav => self.open_wav(),
                Event::Message { text, .. } => {
                    let my = self.rtty.my_call.clone();
                    let text = text.replace("$M", &my).replace("$C", &self.last_call);
                    if self.vox {
                        self.rtty.send(&text);
                    }
                }
                _ => {}
            }
        }
    }

    fn open_wav(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("WAV", &["wav", "WAV"])
            .pick_file()
        else {
            return;
        };
        let played = match self.tab {
            Tab::Rtty => self.rtty.load_wav(&path),
            _ => self.ft.load_wav(&path),
        };
        if let Err(e) = played {
            self.error = e;
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        let title = self.t("settings");
        egui::Window::new(title)
            .open(&mut open)
            .default_width(360.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.settings_body(ui));
            });
        self.settings_open = open;
    }

    fn settings_body(&mut self, ui: &mut egui::Ui) {
        let lang = self.language.clone();
        let t = |k| texts().get(&lang, k);
        if self.inputs.is_empty() {
            self.inputs = input_devices();
            self.outputs = output_devices();
        }
        let shown = |s: &Source| match s {
            Source::Default => t("default_device").to_string(),
            Source::Device(d) => d.clone(),
        };
        let before = self.source.clone();
        rtty_ui::field(ui, t("input"), |ui| {
            egui::ComboBox::from_id_salt("app-input")
                .selected_text(shown(&self.source))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    let mut all = vec![Source::Default];
                    all.extend(self.inputs.iter().cloned().map(Source::Device));
                    for s in all {
                        let label = shown(&s);
                        ui.selectable_value(&mut self.source, s, label);
                    }
                })
                .response
        });
        if self.source != before {
            self.input_io = None;
        }
        let out_before = self.output.clone();
        rtty_ui::field(ui, t("output"), |ui| {
            egui::ComboBox::from_id_salt("app-output")
                .selected_text(self.output.clone().unwrap_or(t("default_device").into()))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.output, None, t("default_device"));
                    for d in &self.outputs {
                        ui.selectable_value(&mut self.output, Some(d.clone()), d);
                    }
                })
                .response
        });
        if self.output != out_before {
            self.output_io = None;
        }
        rtty_ui::field(ui, t("transmit"), |ui| {
            let label = t(if self.vox {
                "transmit.vox"
            } else {
                "transmit.off"
            });
            egui::ComboBox::from_id_salt("app-transmit")
                .selected_text(label)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.vox, false, t("transmit.off"));
                    ui.selectable_value(&mut self.vox, true, t("transmit.vox"));
                })
                .response
        });
        rtty_ui::field(ui, t("language"), |ui| {
            egui::ComboBox::from_id_salt("app-language")
                .selected_text(if self.language == "ja" {
                    "日本語"
                } else {
                    "English"
                })
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.language, "en".into(), "English");
                    ui.selectable_value(&mut self.language, "ja".into(), "日本語");
                })
                .response
        });
        self.ft.language = self.language.clone();
        self.rtty.language = self.language.clone();
        egui::CollapsingHeader::new("FT8 / FT4").show(ui, |ui| self.ft.settings_ui(ui));
        egui::CollapsingHeader::new("RTTY").show(ui, |ui| self.rtty.settings_ui(ui));
        egui::CollapsingHeader::new(t("messages")).show(ui, |ui| {
            for m in &mut self.rtty.messages {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut m.label)
                            .hint_text(t("message.label"))
                            .desired_width(48.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut m.text)
                            .hint_text(t("message.text"))
                            .desired_width(ui.available_width()),
                    );
                });
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.set_theme(if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        self.run_audio();
        egui::Panel::top("app-tabs")
            .frame(
                egui::Frame::side_top_panel(&ctx.global_style())
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Ft8, "FT8");
                    ui.selectable_value(&mut self.tab, Tab::Ft4, "FT4");
                    ui.selectable_value(&mut self.tab, Tab::Rtty, "RTTY");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let tip = self.t("settings");
                        if ui.button(tip).clicked() {
                            self.settings_open = !self.settings_open;
                        }
                        let theme = self.t(if self.dark { "light" } else { "dark" });
                        if ui.button(theme).clicked() {
                            self.dark = !self.dark;
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(8))
            .show(ui, |ui| {
                if !self.error.is_empty() {
                    let e = self.error.clone();
                    if ui
                        .colored_label(egui::Color32::from_rgb(220, 70, 70), e)
                        .clicked()
                    {
                        self.error.clear();
                    }
                }
                match self.tab {
                    Tab::Ft8 | Tab::Ft4 => self.ft.ui(ui),
                    Tab::Rtty => self.rtty.ui(ui),
                }
            });
        if self.tab != Tab::Rtty {
            self.rtty.tick(SystemTime::now());
        } else {
            self.ft.tick(SystemTime::now());
        }
        let mut events = self.ft.take_events();
        events.extend(self.rtty.take_events());
        self.handle(events);
        if self.settings_open {
            self.settings_ui(&ctx);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        for (k, v) in self.ft.settings() {
            storage.set_string(&format!("ft.{k}"), v);
        }
        for (k, v) in self.rtty.settings() {
            storage.set_string(&format!("rtty.{k}"), v);
        }
        for (n, m) in self.rtty.messages.iter().enumerate() {
            storage.set_string(&format!("message{n}.label"), m.label.clone());
            storage.set_string(&format!("message{n}.text"), m.text.clone());
        }
        storage.set_string("app.language", self.language.clone());
        storage.set_string("app.theme", if self.dark { "dark" } else { "light" }.into());
        let mode = match self.tab {
            Tab::Ft8 => "ft8",
            Tab::Ft4 => "ft4",
            Tab::Rtty => "rtty",
        };
        storage.set_string("app.mode", mode.into());
        storage.set_string("app.input", self.source.key());
        storage.set_string("app.output", self.output.clone().unwrap_or_default());
        let tx = if self.vox { "vox" } else { "off" };
        storage.set_string("app.transmit", tx.into());
    }
}

fn system_language() -> String {
    let lang = std::env::var("LANG").unwrap_or_default();
    if lang.starts_with("ja") || cfg!(target_os = "windows") && is_japanese_windows() {
        "ja".into()
    } else {
        "en".into()
    }
}

fn is_japanese_windows() -> bool {
    std::path::Path::new("C:\\Windows\\Fonts\\YuGothR.ttc").exists()
}

fn install_japanese_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    let Some(bytes) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "japanese".into(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("japanese".into());
    }
    ctx.set_fonts(fonts);
}
