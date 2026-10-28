//! Native Android entry point using the actual SolidCraft CAD engine and egui interface.
//! This shares the same SolveApp frontend as desktop and web. Platform-specific file
//! services are deliberately disabled until Android storage access is implemented.
use eframe::egui;
use solidcraft_engine::Session;
use solidcraft_ui_egui::{Services, SolveApp};
use solidcraft_ui_egui::viewport::NavMode;

struct MatePadApp {
    cad: SolveApp,
    touch: TouchNavigation,
}

/// The Android touch driver emulates the primary mouse button for a finger.
/// Preserve short taps for selecting CAD objects and sketch planes; a drag
/// over the empty CAD viewport activates orbit mode.
#[derive(Default)]
struct TouchNavigation {
    finger: Option<egui::TouchId>,
    count: usize,
    start: Option<egui::Pos2>,
    orbiting: bool,
    previous_nav: Option<NavMode>,
    restore_nav: bool,
    defer_pointer_gone: bool,
}

impl TouchNavigation {
    const DRAG_START_POINTS: f32 = 9.0;

    fn input(&mut self, cad: &mut SolveApp, raw: &mut egui::RawInput) {
        // Keep orbit enabled during the release frame so CAD doesn't interpret
        // the end of a drag as an object/plane selection.
        if self.restore_nav {
            cad.viewport.nav = self.previous_nav.take();
            self.restore_nav = false;
            self.orbiting = false;
        }
        if self.defer_pointer_gone {
            raw.events.insert(0, egui::Event::PointerGone);
            self.defer_pointer_gone = false;
        }

        let mut release_touch = false;
        for event in &raw.events {
            if let egui::Event::Touch { id, phase, pos, .. } = event {
                match phase {
                    egui::TouchPhase::Start => {
                        self.count += 1;
                        if self.count == 1 {
                            self.finger = Some(*id);
                            self.start = cad.touch_hits_model_viewport(*pos).then_some(*pos);
                            self.orbiting = false;
                        } else if self.orbiting {
                            cad.viewport.nav = self.previous_nav.take();
                            self.orbiting = false;
                        }
                    }
                    egui::TouchPhase::Move if Some(*id) == self.finger && self.count == 1 => {
                        if !self.orbiting
                            && self.start.is_some_and(|at| at.distance(*pos) >= Self::DRAG_START_POINTS)
                            && cad.viewport.nav.is_none()
                            && cad.tool.is_none()
                            && cad.dialog.is_none()
                            && cad.session.active_sketch.is_none()
                        {
                            self.previous_nav = cad.viewport.nav;
                            cad.viewport.nav = Some(NavMode::Orbit);
                            self.orbiting = true;
                        }
                    }
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        self.count = self.count.saturating_sub(1);
                        if Some(*id) == self.finger {
                            self.finger = None;
                            self.start = None;
                            if self.orbiting {
                                self.restore_nav = true;
                            }
                            release_touch = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if release_touch {
            // egui-winit emits PointerGone in the same frame as the release.
            // Keep the last coordinate visible for CAD's release-time pick.
            raw.events.retain(|ev| !matches!(ev, egui::Event::PointerGone));
            self.defer_pointer_gone = true;
        }
    }
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
        Self { cad, touch: TouchNavigation::default() }
    }
}

impl eframe::App for MatePadApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.cad.logic(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.touch.input(&mut self.cad, raw);
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
