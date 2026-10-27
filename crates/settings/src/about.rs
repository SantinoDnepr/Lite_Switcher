//! The "About" window (`lite-switcher-settings.exe --about`, started from the tray menu).

use eframe::egui;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

use lite_switcher_core::config::Config;
use lite_switcher_core::i18n::Translator;

const SIZE: [f32; 2] = [400.0, 330.0];

struct About {
    tr: Translator,
}

/// The round icon of the program with the letter "L" (the same as in the tray).
fn paint_icon(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let unit = size / 32.0;
    let at = |x: f32, y: f32| rect.min + egui::vec2(x, y) * unit;
    painter.circle_filled(rect.center(), 15.0 * unit, egui::Color32::from_rgb(45, 127, 249));
    let white = egui::Color32::WHITE;
    painter.rect_filled(egui::Rect::from_two_pos(at(10.0, 8.0), at(14.0, 24.0)), 0.0, white);
    painter.rect_filled(egui::Rect::from_two_pos(at(10.0, 20.0), at(22.0, 24.0)), 0.0, white);
}

impl eframe::App for About {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let close = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut close = close;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                paint_icon(ui, 64.0);
                ui.add_space(6.0);
                ui.heading("Lite Switcher");
                ui.label(self.tr.tf("about.version", &[("version", env!("CARGO_PKG_VERSION"))]));
                ui.add_space(10.0);
                ui.label(self.tr.t("about.description"));
                ui.add_space(10.0);
                ui.label(self.tr.t("about.copyright"));
                ui.weak(self.tr.t("about.license"));
                ui.add_space(14.0);
                if ui.button(self.tr.t("buttons.close")).clicked() {
                    close = true;
                }
            });
        });
        if close {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

pub fn run() -> eframe::Result {
    // one About window at a time
    let _mutex = unsafe {
        let handle = CreateMutexW(None, true, w!("Local\\LiteSwitcher.About"));
        if handle.is_err() || GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        handle
    };
    let language = Config::load_or_create().map(|c| c.common.language).unwrap_or_else(|_| "en".into());
    let tr = Translator::new(&language);
    let title = tr.t("about.title").to_string();
    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(SIZE)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_minimize_button(false),
        ..Default::default()
    };
    eframe::run_native(&title, options, Box::new(|_cc| Ok(Box::new(About { tr }))))
}
