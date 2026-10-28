//! Native Android entry point using the actual SolidCraft CAD engine and egui interface.
//! This shares the same SolveApp frontend as desktop and web. Platform-specific file
//! services are deliberately disabled until Android storage access is implemented.
use eframe::egui;
use solidcraft_engine::Session;
use solidcraft_ui_egui::{Services, SolveApp};

struct MatePadApp {
    cad: SolveApp,
}

impl MatePadApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // The default desktop-scale typography was too small on MatePad.
        cc.egui_ctx.set_pixels_per_point(1.6);
        solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
        solidcraft_ui_egui::gpu::install_cpu_picture_renderer();

        let mut cad = SolveApp::new(Session::default(), Services::default());
        if let Some(render_state) = &cc.wgpu_render_state {
            // The Android window has no depth attachment; CAD scene depth is offscreen.
            cad.set_wgpu_split(render_state, 24, 0, 1);
        }
        Self { cad }
    }
}

impl eframe::App for MatePadApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.cad.logic(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.cad.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);
    }
}

#[unsafe(no_mangle)]
fn android_main(android: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    let options = eframe::NativeOptions {
        android_app: Some(android),
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "SolidCraft MatePad",
        options,
        Box::new(|cc| Ok(Box::new(MatePadApp::new(cc)))),
    ) {
        log::error!("Failed to launch SolidCraft: {error}");
    }
}
