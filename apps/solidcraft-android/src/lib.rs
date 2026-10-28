//! First Android NativeActivity milestone: touch-ready prototype.
//! CAD engine integration follows after the native APK boots on a device.
use eframe::egui;

#[derive(Default)]
struct MatePadApp {
    length: f32,
    width: f32,
    height: f32,
    show_grid: bool,
}

impl MatePadApp {
    fn new() -> Self {
        Self { length: 80.0, width: 50.0, height: 35.0, show_grid: true }
    }
}

impl eframe::App for MatePadApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("SolidCraft - MatePad");
            ui.label("Android preview | Native Rust interface");
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.show_grid, "Grid");
                if ui.button("Reset").clicked() {
                    *self = Self::new();
                }
            });
            ui.add(egui::Slider::new(&mut self.length, 10.0..=200.0).text("Length (mm)"));
            ui.add(egui::Slider::new(&mut self.width, 10.0..=200.0).text("Width (mm)"));
            ui.add(egui::Slider::new(&mut self.height, 10.0..=200.0).text("Height (mm)"));
            ui.separator();
            let desired = egui::vec2(ui.available_width(), ui.available_height().max(120.0));
            let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::drag());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(22, 27, 36));
            let center = rect.center();
            let scale = (rect.width() / 320.0).min(rect.height() / 300.0);
            let dx = self.length * scale * 0.75;
            let dy = self.width * scale * 0.40;
            let dz = self.height * scale;
            let a = center + egui::vec2(-dx / 2.0, dy / 2.0);
            let b = a + egui::vec2(dx, 0.0);
            let c = b + egui::vec2(dy, -dy);
            let d = a + egui::vec2(dy, -dy);
            let top = egui::vec2(0.0, -dz);
            let edge = egui::Stroke::new(2.0, egui::Color32::from_rgb(147, 200, 255));
            for (p, q) in [(a,b), (b,c), (c,d), (d,a), (a,a+top), (b,b+top), (c,c+top), (d,d+top),
                            (a+top,b+top), (b+top,c+top), (c+top,d+top), (d+top,a+top)] {
                painter.line_segment([p,q], edge);
            }
            painter.text(
                rect.left_top() + egui::vec2(12.0, 12.0),
                egui::Align2::LEFT_TOP,
                "Parametric box preview (not yet CAD engine)",
                egui::FontId::proportional(14.0),
                egui::Color32::LIGHT_GRAY,
            );
        });
    }
}

#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "SolidCraft MatePad",
        options,
        Box::new(|_cc| Ok(Box::new(MatePadApp::new()))),
    ) {
        log::error!("Failed to launch SolidCraft: {error}");
    }
}
