use std::collections::HashMap;
use std::sync::Arc;

use egui::accesskit::Role;
use egui::ThemePreference;
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, HarnessBuilder};
use egui_system_font_provider::SystemFontProvider;

use rtty_ui::app::{self, Prefs};

fn builder<S>(width: f32, height: f32) -> HarnessBuilder<S> {
    Harness::builder()
        .with_size([width, height])
        .with_context_setup(|ctx| ctx.add_font_provider(Arc::new(SystemFontProvider::new())))
}

#[test]
fn the_theme_is_a_setting_and_persists() {
    let mut h = builder(600.0, 300.0).build_ui_state(
        |ui, p: &mut Prefs| {
            let lang = p.language.clone();
            app::theme_field(ui, &lang, &mut p.theme);
        },
        Prefs::load(|_| None, || "en".into()),
    );
    h.run_steps(2);
    assert_eq!(h.state().theme, ThemePreference::System);
    h.get_by_label("Theme").click();
    h.run_steps(2);
    h.get_by_label("Dark").click();
    h.run_steps(3);
    assert_eq!(h.state().theme, ThemePreference::Dark);
    let mut store = HashMap::new();
    let mut prefs = h.state().clone();
    prefs.language = "ja".into();
    prefs.save(|k, v| {
        store.insert(k.to_string(), v);
    });
    assert_eq!(store["app.theme"], "dark");
    let back = Prefs::load(|k| store.get(k).cloned(), || "en".into());
    assert_eq!(back, prefs);
}

#[test]
fn the_settings_fields_grow_with_the_dialog_width() {
    for w in [280.0, 600.0] {
        let cards = vec!["USB Audio CODEC".to_string()];
        let mut h = builder(w, 300.0).build_ui_state(
            |ui, p: &mut Prefs| {
                app::device_field(ui, "Input", "en", &mut p.input, &cards);
                app::device_field(ui, "Output", "en", &mut p.output, &cards);
                app::language_field(ui, &mut p.language);
                app::theme_field(ui, "en", &mut p.theme);
            },
            Prefs::load(|_| None, || "en".into()),
        );
        h.run_steps(2);
        let combos = h.get_all_by_role(Role::ComboBox).collect::<Vec<_>>();
        assert_eq!(combos.len(), 4);
        for c in combos {
            let r = c.rect();
            assert!(r.max.x > w - 24.0 && r.width() > w * 0.55, "{r:?} at {w}");
        }
    }
}

#[test]
fn a_host_that_keys_ptt_hides_the_ptt_delays() {
    for host in [false, true] {
        let mut panel = rtty_ui::RttyPanel::new("en");
        panel.host_ptt = host;
        let keys: Vec<_> = panel.settings().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys.contains(&"ptt_before"), !host);
        assert_eq!(keys.contains(&"ptt_after"), !host);
        assert!(keys.contains(&"ptt"));
        let mut h = builder(600.0, 900.0)
            .build_ui_state(|ui, p: &mut rtty_ui::RttyPanel| p.settings_ui(ui), panel);
        h.run_steps(2);
        assert_eq!(h.query_by_label_contains("PTT before").is_some(), !host);
    }
}
