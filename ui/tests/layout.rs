use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use std::time::{Duration, SystemTime};

use rtty_ui::{Bank, Event, Message, RttyPanel};

const START: &str = "Start";
const STOP: &str = "Stop";
const OPEN_WAV: &str = "Open…";
const STOP_WAV: &str = "Stop WAV";
const SETTINGS: &str = "Settings";

fn builder<S>() -> egui_kittest::HarnessBuilder<S> {
    Harness::builder()
}

const WIDTHS: [f32; 3] = [280.0, 360.0, 600.0];

const CONTROLS: [Role; 6] = [
    Role::Button,
    Role::RadioButton,
    Role::TextInput,
    Role::SpinButton,
    Role::CheckBox,
    Role::Slider,
];

fn inside<S>(h: &Harness<'_, S>, width: f32) {
    let mut n = 0;
    for node in h.query_all_by(|n| CONTROLS.contains(&n.role())) {
        let r = node.rect();
        assert!(
            r.min.x >= -0.5 && r.max.x <= width + 0.5,
            "{:?} {:?} at {width}",
            node.accesskit_node().label(),
            r
        );
        n += 1;
    }
    assert!(n > 0);
    assert!(h.query_by_role(Role::ComboBox).is_none());
}

fn in_order<S>(h: &Harness<'_, S>, labels: &[&str]) {
    let rects: Vec<egui::Rect> = labels.iter().map(|l| h.get_by_label(l).rect()).collect();
    for (w, l) in rects.windows(2).zip(labels.windows(2)) {
        let (a, b) = (w[0], w[1]);
        let same_row = (a.center().y - b.center().y).abs() < 2.0;
        assert!(
            (same_row && a.max.x <= b.min.x) || a.max.y <= b.min.y + 1.0,
            "{l:?} {a:?} {b:?}"
        );
    }
}

fn right_top<S>(h: &Harness<'_, S>, width: f32) {
    let s = h.get_by_label(SETTINGS).rect();
    let start = h.get_by_label(START).rect();
    assert!(s.max.x > width - 12.0 && s.min.y < 20.0, "{s:?} at {width}");
    assert!(start.max.x <= s.min.x || start.max.y <= s.min.y + 1.0);
}

fn rtty(width: f32, panel: RttyPanel) -> Harness<'static, RttyPanel> {
    let mut h = builder()
        .with_size([width, 480.0])
        .build_ui_state(|ui, p: &mut RttyPanel| p.ui(ui), panel);
    h.run_steps(3);
    h
}

fn messages() -> RttyPanel {
    let mut p = RttyPanel::new("en");
    p.messages = ["CQ", "Call", "599", "TU", "QRZ", "AGN"]
        .map(|l| Message::new(l, &format!("{l} DE JA1ZLO")))
        .to_vec();
    p.sp_messages = ["My", "Rpt", "NR?"]
        .map(|l| Message::new(l, &format!("{l} JA1ZLO")))
        .to_vec();
    p
}

#[test]
fn the_rtty_panel_fits_every_width_with_its_controls_in_order() {
    for w in WIDTHS {
        let h = rtty(w, messages());
        inside(&h, w);
        in_order(
            &h,
            &[
                START,
                OPEN_WAV,
                "CQ messages",
                "CQ",
                "Call",
                "599",
                "TU",
                "QRZ",
                "AGN",
                "Send",
                "Halt",
            ],
        );
        right_top(&h, w);
        assert!(h.query_by_label("Mark").is_none() && h.query_by_label("Light").is_none());
        let field = h.get_by_role(Role::TextInput).rect();
        assert!(field.width() > w * 0.4, "the text field stretches at {w}");
    }
}

#[test]
fn hidden_tools_are_not_drawn() {
    let mut p = messages();
    p.tools = rtty_ui::RttyTools::NONE;
    let h = rtty(360.0, p);
    assert!(h.query_by_label("CQ").is_none() && h.query_by_label(START).is_none());
    assert!(h.query_by_label(SETTINGS).is_none());
    assert!(h.query_by_label("CQ messages").is_none());
}

#[test]
fn the_toolbar_switches_between_the_cq_and_sp_banks() {
    let mut h = rtty(360.0, messages());
    assert!(h.query_by_label("QRZ").is_some() && h.query_by_label("NR?").is_none());
    h.get_by_label("CQ messages").click();
    h.run_steps(2);
    assert_eq!(h.state().bank, Bank::Sp);
    assert!(h.query_by_label("QRZ").is_none());
    h.get_by_label("NR?").click();
    h.run_steps(2);
    let e = h.state_mut().take_events();
    assert!(e.contains(&Event::Message {
        bank: Bank::Sp,
        index: 2,
        text: "NR? JA1ZLO".into()
    }));
    h.get_by_label("S&P messages").click();
    h.run_steps(2);
    assert_eq!(h.state().bank, Bank::Cq);
    let mut b = RttyPanel::new("en");
    for (k, v) in h.state().settings() {
        b.load_setting(k, &v);
    }
    assert_eq!(b.settings(), h.state().settings());
}

#[test]
fn the_rtty_panel_works_in_a_tall_narrow_dock() {
    let mut p = messages();
    let now = SystemTime::now();
    p.simulate("CQ TEST DE JA1ZLO JA1ZLO", now);
    p.tick(now + Duration::from_secs(20));
    p.stop();
    let mut h = builder()
        .with_size([280.0, 800.0])
        .build_ui_state(|ui, p: &mut RttyPanel| p.ui(ui), p);
    h.run_steps(3);
    inside(&h, 280.0);
    right_top(&h, 280.0);
    let term = h.get_by_label("Terminal").rect();
    let send = h.get_by_label("Send").rect();
    assert!(
        term.height() > 500.0 && send.max.y <= 800.0,
        "{term:?} {send:?}"
    );
}

#[test]
fn start_toggles_and_a_remote_panel_speaks_commands() {
    let mut h = rtty(600.0, messages());
    h.get_by_label(START).click();
    h.run_steps(2);
    assert!(h.state().running());
    h.get_by_label(STOP).click();
    h.run_steps(2);
    assert!(!h.state().running());
    let mut engine = rtty::Engine::new();
    let mut p = RttyPanel::remote("en");
    p.options.mark = 1500.0;
    p.start();
    p.send("cq test");
    let sent = p.take_commands();
    assert!(matches!(sent[0], rtty::Command::Options(o) if o.mark == 1500.0));
    assert!(matches!(&sent[2], rtty::Command::Send { text, .. } if text == "\r\nCQ TEST\r\n"));
    assert!(!p.running() && !p.sending());
    for c in sent {
        engine.handle(c);
    }
    for n in engine.take() {
        p.notify(n);
    }
    assert!(p.running() && p.sending() && engine.options().mark == 1500.0);
    assert!(p.take_events().contains(&Event::Ptt(true)));
}

fn wav_file(name: &str, audio: &[f32], rate: u32) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rtty-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    rtty_ui::wav::write(&path, audio, rate).unwrap();
    path
}

#[test]
fn open_wav_asks_the_host_and_a_chosen_file_plays_into_the_decoder() {
    let now = SystemTime::now();
    let options = rtty::RttyOptions::default();
    let band = rtty::sim::Band::audio(12000, 6.0);
    let station = rtty::sim::Station::new("\r\nCQ TEST DE JA1ZLO\r\n", options.mark, 0.0, 15.0);
    let audio = rtty::sim::render(&band, rtty::sim::Source::Rtty(options), &[station], 1).unwrap();
    let path = wav_file("rtty.wav", &audio, 12000);
    let mut h = rtty(280.0, messages());
    inside(&h, 280.0);
    h.get_by_label(OPEN_WAV).click();
    h.run_steps(2);
    assert!(h
        .state_mut()
        .take_events()
        .contains(&rtty_ui::Event::OpenWav));
    h.state_mut().load_wav(&path).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label(STOP_WAV).is_some());
    h.state_mut().tick(now);
    h.state_mut().tick(now + Duration::from_secs(10));
    h.run_steps(2);
    let lines: Vec<&str> = h.state().terminal().all_lines().map(str::trim).collect();
    assert!(lines.contains(&"CQ TEST DE JA1ZLO"), "{lines:?}");
    assert_eq!(h.state().terminal().calls, ["JA1ZLO"]);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn settings_fields_fill_the_dialog_width() {
    for w in [280.0, 600.0] {
        let mut h = builder().with_size([w, 700.0]).build_ui_state(
            |ui, p: &mut RttyPanel| p.settings_ui(ui),
            RttyPanel::new("en"),
        );
        h.run_steps(2);
        for f in h.get_all_by_role(Role::TextInput) {
            let r = f.rect();
            assert!(r.max.x <= w + 0.5 && r.max.x > w - 24.0, "{r:?} at {w}");
        }
        for c in h.get_all_by_role(Role::ComboBox) {
            assert!(c.rect().max.x <= w + 0.5 && c.rect().max.x > w - 24.0);
        }
    }
}

#[test]
fn the_terminal_fills_the_width() {
    for w in WIDTHS {
        let mut p = messages();
        let now = SystemTime::now();
        p.simulate("CQ TEST DE JA1ZLO JA1ZLO JH1ABC JR1XYZ", now);
        p.tick(now + Duration::from_secs(20));
        let h = rtty(w, p);
        assert!(!h.state().terminal().calls.is_empty());
        let panel = w - 16.0;
        let term = h.get_by_label("Terminal").rect();
        assert!(term.width() >= 0.95 * panel, "{term:?} at {w}");
        assert!(term.height() > 240.0, "{term:?} at {w}");
        let call = h
            .get_all_by_label("JA1ZLO")
            .find(|n| n.accesskit_node().role() == Role::Button)
            .unwrap()
            .rect();
        assert!(
            call.min.y >= term.max.y,
            "the callsigns sit below the terminal"
        );
        inside(&h, w);
    }
}
