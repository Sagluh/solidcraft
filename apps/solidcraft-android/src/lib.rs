//! Native Android entry point using the actual SolidCraft CAD engine and egui interface.
//! This shares the same SolveApp frontend as desktop and web. Platform-specific file
//! services are deliberately disabled until Android storage access is implemented.
mod android_files;

use android_files::AndroidFiles;
use eframe::egui;
use solidcraft_engine::Session;
use solidcraft_ui_egui::SolveApp;
use solidcraft_ui_egui::viewport::NavMode;

struct MatePadApp {
    cad: SolveApp,
    touch: TouchNavigation,
    files: AndroidFiles,
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
    fn new(cc: &eframe::CreationContext<'_>, android: android_activity::AndroidApp) -> Self {
        // The default desktop-scale typography was too small on MatePad.
        cc.egui_ctx.set_pixels_per_point(1.6);
        solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
        solidcraft_ui_egui::gpu::install_cpu_picture_renderer();

        let files = AndroidFiles::new(android.clone());
        let mut cad = SolveApp::new(Session::default(), android_files::services(&android));
        if let Some(render_state) = &cc.wgpu_render_state {
            // The Android window has no depth attachment; CAD scene depth is offscreen.
            cad.set_wgpu_split(render_state, 24, 0, 1);
        }
        Self { cad, touch: TouchNavigation::default(), files }
    }
}

impl eframe::App for MatePadApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Android provides Touch events for both fingers. Unlike desktop
        // mouse input, these are not forwarded as scroll/zoom wheel events.
        // Apply the genuine two-finger gesture to the CAD camera before
        // updating the rest of the application.
        if let Some(gesture) = ctx.input(|input| input.multi_touch())
            && self.cad.touch_hits_model_viewport(gesture.center_pos)
        {
            let zoom = (gesture.zoom_delta as f64).clamp(0.6, 1.7);
            if zoom.is_finite() && (zoom - 1.0).abs() > 0.0001 {
                self.cad.cancel_view_animation();
                self.cad.cam.zoom_at(1.0 / zoom, None);
            }
            let delta = gesture.translation_delta;
            if delta.is_finite() && delta != egui::Vec2::ZERO {
                self.cad.cancel_view_animation();
                let height = self.cad.viewport.rect.map_or(600.0, |rect| rect.height().max(1.0));
                self.cad.cam.pan(delta.x as f64, delta.y as f64, height as f64);
            }
            if (zoom - 1.0).abs() > 0.0001 || delta != egui::Vec2::ZERO {
                ctx.request_repaint();
            }
        }
        self.cad.logic(ctx);
        self.files.tick(&mut self.cad);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.touch.input(&mut self.cad, raw);
        self.cad.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);

        // Large, explicit plane targets: a finger shouldn't have to hit the
        // tiny 3D origin squares to start sketching. The original plane/face
        // picker remains available behind this touch-only helper.
        if self.cad.picking_sketch_plane() {
            egui::Area::new(egui::Id::new("matepad_sketch_planes"))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 92.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::default()
                        .fill(egui::Color32::from_rgb(30, 37, 48))
                        .inner_margin(12.0)
                        .corner_radius(10.0)
                        .show(ui, |ui| {
                            ui.heading("Плоскость эскиза");
                            ui.label("Нажмите XY, XZ, YZ или коснитесь грани модели");
                            ui.add_space(7.0);
                            ui.horizontal(|ui| {
                                for name in ["XY", "XZ", "YZ"] {
                                    if ui.add_sized([94.0, 48.0], egui::Button::new(name)).clicked() {
                                        let plane = solidcraft_engine::Sel::Plane { name: name.to_owned() };
                                        match solidcraft_ui_egui::dialogs::start_sketch(&mut self.cad, &plane) {
                                            Ok(()) => self.cad.dialog = None,
                                            Err(message) => self.cad.set_status(message, true),
                                        }
                                    }
                                }
                            });
                        });
                });
        }
    }
}

#[unsafe(no_mangle)]
fn android_main(android: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    let options = eframe::NativeOptions {
        android_app: Some(android.clone()),
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "SolidCraft MatePad",
        options,
        Box::new(move |cc| Ok(Box::new(MatePadApp::new(cc, android.clone())))),
    ) {
        log::error!("Failed to launch SolidCraft: {error}");
    }
}
