//! Native Android entry point using the actual SolidCraft CAD engine and egui interface.
//! This shares the same SolveApp frontend as desktop and web. Platform-specific file
//! services use Android's system document picker for import and export.
mod android_files;

use android_files::AndroidFiles;
use eframe::egui;
use solidcraft_engine::{Sel, Session};
use solidcraft_engine::geom::Vec3;
use serde_json::json;
use solidcraft_ui_egui::SolveApp;
use solidcraft_ui_egui::viewport::NavMode;

/// Translation with two fingers either orbits the model freely around the
/// view's X/Y axes or pans it. Pinch always zooms. Finger twist rolls around Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TwoFingerMode {
    #[default]
    Rotate3D,
    Pan,
}

struct MatePadApp {
    cad: SolveApp,
    touch: TouchNavigation,
    files: AndroidFiles,
    two_finger_mode: TwoFingerMode,
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
    /// True from the second finger's down event until the final finger lifts.
    /// While set, synthetic mouse events must never select CAD geometry.
    multi_touch_active: bool,
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
        let was_multi_touch = self.multi_touch_active;
        let mut began_multi_touch = false;
        for event in &raw.events {
            if let egui::Event::Touch { id, phase, pos, .. } = event {
                match phase {
                    egui::TouchPhase::Start => {
                        self.count += 1;
                        if self.count == 1 {
                            self.finger = Some(*id);
                            self.start = cad.touch_hits_model_viewport(*pos).then_some(*pos);
                            self.orbiting = false;
                        } else if self.count >= 2 && !self.multi_touch_active {
                            // Android's touch backend also emits mouse events for one
                            // of the fingers. Disable that emulated mouse for the full
                            // gesture, even after only one finger remains.
                            self.multi_touch_active = true;
                            began_multi_touch = true;
                            self.restore_nav = false;
                            if self.orbiting {
                                cad.viewport.nav = self.previous_nav.take();
                                self.orbiting = false;
                            }
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
        if self.multi_touch_active || was_multi_touch {
            // Keep the real Touch events: egui needs them to calculate pinch
            // zoom and two-finger translation. Discard the *emulated mouse*
            // events so the first finger cannot click, box-select or move a
            // body while both fingers are manipulating the camera.
            // Importantly this includes the release of the LAST finger.
            self.defer_pointer_gone = false;
            raw.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerGone
                )
            });
            if began_multi_touch {
                // The first finger may already have pressed the primary mouse
                // button in an earlier frame. Release it *outside* the app
                // so egui cancels the drag without generating a CAD click.
                let outside = egui::pos2(-10_000.0, -10_000.0);
                raw.events.insert(0, egui::Event::PointerGone);
                raw.events.insert(0, egui::Event::PointerButton {
                    pos: outside,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                raw.events.insert(0, egui::Event::PointerMoved(outside));
            }
            if self.count == 0 {
                self.multi_touch_active = false;
            }
        } else if release_touch {
            // Short one-finger taps must retain their position until the CAD
            // plane/face selection has handled the release.
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
        // Android MatePad edition starts in Russian regardless of the system
        // locale. This is an Android-only preference; desktop remains unchanged.
        // Users can still select another language in Preferences for this run.
        cad.preferences.language = "ru".to_owned();
        if let Some(render_state) = &cc.wgpu_render_state {
            // The Android window has no depth attachment; CAD scene depth is offscreen.
            cad.set_wgpu_split(render_state, 24, 0, 1);
        }
        Self { cad, touch: TouchNavigation::default(), files, two_finger_mode: TwoFingerMode::default() }
    }

    /// A small toggle above the viewport navigation bar: two fingers can
    /// rotate all three axes or perform the previous two-finger translation.
    /// Zoom by pinching works in either mode; regular CAD taps are unchanged.
    fn two_finger_mode_button(&mut self, ctx: &egui::Context) {
        if self.cad.session.active_sketch.is_some() {
            return; // Keep the 2D sketch plane stable and use two fingers for pan/zoom.
        }
        egui::Area::new(egui::Id::new("matepad_two_finger_camera_mode"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -101.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(29, 38, 51))
                    .inner_margin(egui::Margin::symmetric(8, 5))
                    .corner_radius(9.0)
                    .show(ui, |ui| {
                        let (label, hint) = match self.two_finger_mode {
                            TwoFingerMode::Rotate3D => (
                                "2 пальца: Вращение 3D ↻",
                                "Двигайте два пальца для поворота по X/Y, закручивайте их для вращения вокруг оси взгляда. Щипок — масштаб.",
                            ),
                            TwoFingerMode::Pan => (
                                "2 пальца: Перемещение ↔",
                                "Два пальца перемещают модель по экрану. Щипок — масштаб. Нажмите для перехода к свободному вращению.",
                            ),
                        };
                        if ui.add_sized([235.0, 36.0], egui::Button::new(label))
                            .on_hover_text(hint)
                            .clicked()
                            && !self.touch.multi_touch_active
                        {
                            self.two_finger_mode = match self.two_finger_mode {
                                TwoFingerMode::Rotate3D => TwoFingerMode::Pan,
                                TwoFingerMode::Pan => TwoFingerMode::Rotate3D,
                            };
                        }
                    });
            });
    }

    /// Android sketch editing controls: select/drag by touch without needing
    /// Escape, a hardware mouse, or a keyboard shortcut. Fix is OPTIONAL:
    /// normal unpinned geometry remains editable until the user locks it.
    fn sketch_touch_toolbar(&mut self, ctx: &egui::Context) {
        let Some(sid) = self.cad.session.active_sketch else { return };
        let drawing = self.cad.tool.is_some();
        let selections: Vec<String> = self.cad.session.selection.iter().filter_map(|item| match item {
            Sel::SketchCurve { id, .. } | Sel::SketchPoint { id } => Some(id.clone()),
            _ => None,
        }).collect();
        // Avoid scanning and allocating all sketch curves every rendered frame.
        // Large engineering sketches may contain thousands of elements.
        let all_locked = if selections.is_empty() {
            false
        } else {
            let world = self.cad.session.world_state();
            let Some(sketch) = world.sketch(sid).map(|ss| &ss.sketch) else { return };
            selections.iter().all(|id| {
                sketch.curve_index(id).is_some_and(|i| sketch.curve_locked(i))
                    || sketch.resolve_point(id).is_some_and(|i| sketch.point_locked(i))
            })
        };
        #[derive(Clone, Copy)]
        enum Action { Select, SelectAll, Fix, Move }
        let mut action: Option<Action> = None;
        egui::Area::new(egui::Id::new("matepad_sketch_edit_tools"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -150.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(29, 38, 51))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if drawing {
                                if ui.add_sized([176.0, 46.0], egui::Button::new("Выбрать / двигать")).clicked() {
                                    action = Some(Action::Select);
                                }
                            } else {
                                if ui.add_sized([125.0, 46.0], egui::Button::new("Выделить всё")).clicked() {
                                    action = Some(Action::SelectAll);
                                }
                                if !selections.is_empty() {
                                    if ui.add_sized([136.0, 46.0], egui::Button::new(
                                        if all_locked { "Освободить" } else { "Закрепить" }
                                    )).clicked() {
                                        action = Some(Action::Fix);
                                    }
                                    if ui.add_sized([130.0, 46.0], egui::Button::new("Переместить…")).clicked() {
                                        action = Some(Action::Move);
                                    }
                                }
                            }
                        });
                    });
            });
        match action {
            Some(Action::Select) => {
                // Dropping the active drawing tool leaves already committed
                // geometry intact; its unfinished preview is discarded.
                self.cad.tool = None;
                self.cad.set_status("Режим выбора: коснитесь линии или перетащите её".to_owned(), false);
            }
            Some(Action::SelectAll) => {
                let world = self.cad.session.world_state();
                let Some(sketch) = world.sketch(sid).map(|ss| &ss.sketch) else { return };
                let all: Vec<Sel> = sketch.curves.iter().filter(|c| c.link.is_none())
                    .map(|c| Sel::SketchCurve { id: c.id.clone(), sketch: None })
                    .collect();
                if let Err(error) = self.cad.run("select.set", json!({"items":all,"add":false})) {
                    self.cad.set_status(error.to_string(), true);
                }
            }
            Some(Action::Fix) => {
                // Use an explicit desired state, not an implicit toggle, so a
                // mixed selection is reliably locked together or released.
                let desired = !all_locked;
                if let Err(error) = self.cad.run("sketch.constraint.fix",
                    json!({"entities":selections,"fixed":desired}))
                {
                    self.cad.set_status(error.to_string(), true);
                }
            }
            Some(Action::Move) => {
                self.cad.start("sketch.move");
            }
            None => {}
        }
    }
}

impl eframe::App for MatePadApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Android provides Touch events for both fingers. Unlike desktop
        // mouse input, these are not forwarded as scroll/zoom wheel events.
        // With two fingers on the CAD viewport:
        // * pinch       -> zoom (both modes)
        // * translate   -> free 3D orbit (default) or pan (user-selected)
        // * finger twist-> roll around the viewing direction (3rd rotation axis)
        // Free rotation uses Camera::rotate_in_view, which preserves orientation
        // across the poles; yaw/pitch Camera::orbit would clamp there.
        //
        // Android's synthetic primary-mouse events were already removed in
        // TouchNavigation::input, so these gestures cannot select model faces.
        if let Some(gesture) = ctx.input(|input| input.multi_touch())
            && self.cad.touch_hits_model_viewport(gesture.center_pos)
        {
            let zoom = (gesture.zoom_delta as f64).clamp(0.6, 1.7);
            let pinch = zoom.is_finite() && (zoom - 1.0).abs() > 0.0001;
            let delta = gesture.translation_delta;
            let translation = delta.is_finite() && delta != egui::Vec2::ZERO;
            let twist = f64::from(gesture.rotation_delta);
            let twisting = twist.is_finite() && twist.abs() > 0.0001;
            let free_orbit = self.two_finger_mode == TwoFingerMode::Rotate3D
                && self.cad.session.active_sketch.is_none();
            if pinch || translation || (twisting && free_orbit) {
                self.cad.cancel_view_animation();
                self.cad.viewport.orbit_free = None;
                self.cad.viewport.orbit_from = None;
                self.cad.viewport.spin = false;
                if pinch {
                    self.cad.cam.zoom_at(1.0 / zoom, None);
                }
                if free_orbit {
                    // Screen-space trackball: vertical movement rotates around
                    // camera-right, horizontal around camera-up, and a two-
                    // finger twist around camera-back. Clamp outlier frame
                    // deltas to avoid a jump at touch pointer acquisition.
                    let dx = f64::from(delta.x).clamp(-100.0, 100.0);
                    let dy = f64::from(delta.y).clamp(-100.0, 100.0);
                    let roll = if twisting { twist.clamp(-0.35, 0.35) } else { 0.0 };
                    self.cad.cam.rotate_in_view(Vec3::new(-dy * 0.008, -dx * 0.008, -roll));
                } else if translation {
                    // Preserve existing two-finger pan and 2D sketch navigation.
                    let height = self.cad.viewport.rect.map_or(600.0, |rect| rect.height().max(1.0));
                    self.cad.cam.pan(delta.x as f64, delta.y as f64, height as f64);
                }
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
        self.sketch_touch_toolbar(ui.ctx());
        self.two_finger_mode_button(ui.ctx());

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
