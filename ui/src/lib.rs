pub mod app;
#[cfg(feature = "app")]
pub mod audio;
pub mod clock;
pub mod console;
pub mod event;
pub mod heard;
pub mod icon;
pub mod terminal;
pub mod text;
pub mod wav;

pub use clock::Clock;
pub use console::{Bank, Message, RttyPanel, RttyTools};
pub use event::Event;
pub use heard::Heard;
pub use rtty::{Burst, Pick, Timing};
pub use terminal::Terminal;

use egui::{Color32, Ui, Visuals};

const LABEL_WIDTH: f32 = 96.0;

pub(crate) fn accent(v: &Visuals) -> Color32 {
    if v.dark_mode {
        Color32::from_rgb(110, 170, 255)
    } else {
        Color32::from_rgb(20, 80, 200)
    }
}

pub(crate) fn alert(v: &Visuals) -> Color32 {
    if v.dark_mode {
        Color32::from_rgb(240, 100, 90)
    } else {
        Color32::from_rgb(190, 30, 30)
    }
}

pub(crate) fn good(v: &Visuals) -> Color32 {
    if v.dark_mode {
        Color32::from_rgb(110, 210, 120)
    } else {
        Color32::from_rgb(20, 130, 40)
    }
}

pub fn field(
    ui: &mut Ui,
    label: &str,
    add: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    ui.horizontal(|ui| {
        let width = LABEL_WIDTH.min(ui.available_width() * 0.35);
        let h = ui.spacing().interact_size.y;
        let id = ui
            .allocate_ui_with_layout(
                egui::vec2(width, h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(width);
                    ui.add(egui::Label::new(label).truncate()).id
                },
            )
            .inner;
        let rest = ui.available_width();
        ui.spacing_mut().text_edit_width = rest;
        ui.spacing_mut().slider_width = (rest - 60.0).max(40.0);
        ui.spacing_mut().combo_width = rest;
        add(ui).labelled_by(id)
    })
    .inner
}

pub fn bar(ui: &mut Ui, right: impl FnOnce(&mut Ui), left: impl FnOnce(&mut Ui)) {
    let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    let layout = egui::Layout::right_to_left(egui::Align::Min);
    ui.allocate_ui_with_layout(row, layout, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        right(ui);
        let row = egui::vec2(ui.available_width(), row.y);
        let wrap = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
        ui.allocate_ui_with_layout(row, wrap, |ui| {
            ui.set_row_height(row.y);
            left(ui)
        });
    });
}

pub fn flags(ui: &mut Ui, flags: &mut [(&mut bool, &str)]) {
    ui.horizontal_wrapped(|ui| {
        for (v, label) in flags.iter_mut() {
            ui.checkbox(v, *label);
        }
    });
}

pub fn text_row<'a>(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    buttons: &[&'a str],
) -> Vec<&'a str> {
    let mut clicked = Vec::new();
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        for b in buttons.iter().rev() {
            if ui.button(*b).clicked() {
                clicked.push(*b);
            }
        }
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .desired_width(ui.available_width()),
        );
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(b) = buttons.first() {
                clicked.push(*b);
            }
            r.request_focus();
        }
    });
    clicked
}
