//! A compact in-app numerical keyboard for Android MatePad CAD dimensions.
//!
//! Native Android IME occupies much of the modelling viewport. For numerical
//! expressions we instead present an egui touch keypad by the requested value.
//! The regular system keyboard remains an explicit option for advanced formulas.
//! This module is Android-only and does not change the desktop input pipeline.
#![cfg(target_os = "android")]

use egui::{Button, Context, Id, Order, Pos2, Rect, Response, RichText, Ui, Vec2, pos2};

#[derive(Clone)]
struct Active {
    target: Id,
    initial_pos: Pos2,
    hint: String,
    original: String,
    draft: String,
    replace: bool,
    system_keyboard: bool,
    request_native_focus: bool,
}

#[derive(Clone, Copy)]
enum Key {
    Type(&'static str),
    Backspace,
    Clear,
    Sign,
    Accept,
    Cancel,
    Native,
}

fn pad_id() -> Id {
    Id::new("solidcraft_android_numeric_pad")
}

fn confirmed_id() -> Id {
    Id::new("solidcraft_android_numeric_confirmed")
}

/// Used by the sketch dimension and feature-dialog handlers when the user
/// confirms the custom keypad rather than pressing the hardware Enter key.
pub(crate) fn take_confirm(ctx: &Context) -> bool {
    ctx.data_mut(|data| {
        let yes = data.get_temp::<bool>(confirmed_id()).unwrap_or(false);
        data.remove::<bool>(confirmed_id());
        yes
    })
}

fn set_confirm(ctx: &Context) {
    ctx.data_mut(|data| data.insert_temp(confirmed_id(), true));
}

fn action_button(ui: &mut Ui, caption: &str, size: [f32; 2], action: Key, output: &mut Option<Key>) {
    if ui.add_sized(size, Button::new(caption)).clicked() {
        *output = Some(action);
    }
}

fn popup_position(ctx: &Context, field: Rect) -> Pos2 {
    const WIDTH: f32 = 250.0;
    const HEIGHT: f32 = 318.0;
    let view = ctx.content_rect();
    // Normally the pad opens beside the field, away from the model. If the
    // command panel is at the edge, put it on the other side of that panel.
    let x = if field.right() + WIDTH + 12.0 <= view.right() {
        field.right() + 8.0
    } else {
        field.left() - WIDTH - 8.0
    };
    let x = x.clamp(view.left() + 8.0, (view.right() - WIDTH - 8.0).max(view.left() + 8.0));
    let y = (field.center().y - HEIGHT / 2.0)
        .clamp(view.top() + 8.0, (view.bottom() - HEIGHT - 8.0).max(view.top() + 8.0));
    pos2(x, y)
}

fn draw_pad(ctx: &Context, active: &Active) -> Option<Key> {
    let mut action = None;
    egui::Area::new(Id::new("solidcraft_android_number_keyboard").with(active.target))
        .default_pos(active.initial_pos)
        .movable(true)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgb(29, 37, 50))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_min_width(230.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Перетащите · {}", if active.hint.is_empty() { "Значение" } else { &active.hint })).strong());
                        if ui.small_button("✕").clicked() {
                            action = Some(Key::Cancel);
                        }
                    });
                    ui.label(RichText::new(if active.draft.is_empty() { "0" } else { &active.draft })
                        .monospace().size(19.0));
                    ui.separator();
                    let key_size = [50.0, 40.0];
                    for row in [
                        [("7", Key::Type("7")), ("8", Key::Type("8")), ("9", Key::Type("9")), ("Удал", Key::Backspace)],
                        [("4", Key::Type("4")), ("5", Key::Type("5")), ("6", Key::Type("6")), ("C", Key::Clear)],
                        [("1", Key::Type("1")), ("2", Key::Type("2")), ("3", Key::Type("3")), ("±", Key::Sign)],
                        [("0", Key::Type("0")), (".", Key::Type(".")), ("−", Key::Type("-")), ("+", Key::Type("+"))],
                        [("×", Key::Type("*")), ("÷", Key::Type("/")), ("мм", Key::Type(" mm")), ("°", Key::Type(" deg"))],
                    ] {
                        ui.horizontal(|ui| {
                            for (text, key) in row {
                                action_button(ui, text, key_size, key, &mut action);
                            }
                        });
                    }
                    ui.add_space(3.0);
                    ui.horizontal(|ui| {
                        action_button(ui, "Формула ⌨", [104.0, 42.0], Key::Native, &mut action);
                        action_button(ui, "Готово ✓", [110.0, 42.0], Key::Accept, &mut action);
                    });
                });
        });
    action
}

fn apply_key(active: &mut Active, key: Key) {
    match key {
        Key::Type(text) => {
            if active.replace {
                active.draft.clear();
                active.replace = false;
            }
            if active.draft.len().saturating_add(text.len()) <= 4096 {
                active.draft.push_str(text);
            }
        }
        Key::Backspace => {
            if active.replace {
                active.draft.clear();
            } else {
                active.draft.pop();
            }
            active.replace = false;
        }
        Key::Clear => {
            active.draft.clear();
            active.replace = false;
        }
        Key::Sign => {
            if let Some(unsigned) = active.draft.strip_prefix('-') {
                active.draft = unsigned.to_owned();
            } else {
                active.draft.insert(0, '-');
            }
            active.replace = false;
        }
        Key::Accept | Key::Cancel | Key::Native => {}
    }
}

/// Open a compact numeric pad as soon as the first dimension of a newly
/// started sketch segment appears. Called once per drawing stage transition,
/// not on every repaint. The pad's position stays fixed while the line moves.
pub(crate) fn open_for_sketch(ctx: &Context, id: Id, value: &str, hint: &str, field_rect: Rect) {
    let initial_pos = popup_position(ctx, field_rect);
    ctx.data_mut(|data| data.insert_temp(pad_id(), Active {
        target: id,
        initial_pos,
        hint: hint.to_owned(),
        original: value.to_owned(),
        draft: value.to_owned(),
        replace: true,
        system_keyboard: false,
        request_native_focus: false,
    }));
    ctx.request_repaint();
}

/// Renders a value box that behaves like a TextEdit to its owner (including
/// Response::changed), but never gives Android's IME focus unless the user
/// explicitly taps "Формула ⌨".
///
/// Return: (response, keypad "Готово" pressed).
pub(crate) fn value_field(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    size: Vec2,
    explicit_id: Option<Id>,
) -> (Response, bool) {
    let ctx = ui.ctx().clone();
    let current = ctx.data(|data| data.get_temp::<Active>(pad_id()));
    if let Some(state) = current.as_ref().filter(|state| state.system_keyboard && explicit_id == Some(state.target)) {
        let editor = egui::TextEdit::singleline(value)
            .id(state.target)
            .desired_width(size.x)
            .hint_text(hint);
        let response = ui.add_sized(size, editor);
        if state.request_native_focus {
            response.request_focus();
            let mut updated = state.clone();
            updated.request_native_focus = false;
            ctx.data_mut(|data| data.insert_temp(pad_id(), updated));
        }
        if response.lost_focus() {
            ctx.data_mut(|data| data.remove::<Active>(pad_id()));
        }
        return (response, false);
    }

    let label = if value.trim().is_empty() { hint } else { value.as_str() };
    let button = Button::new(RichText::new(label).monospace()).min_size(size);
    // A push_id makes the fields with an explicit ID stable. Auto-ID buttons
    // are stable in the same widget tree across ordinary CAD preview updates.
    let mut response = if let Some(id) = explicit_id {
        ui.push_id(id, |ui| ui.add_sized(size, button)).inner
    } else {
        ui.add_sized(size, button)
    };
    let target = explicit_id.unwrap_or(response.id);
    let mut state = ctx.data(|data| data.get_temp::<Active>(pad_id()));
    if response.clicked() {
        state = Some(Active {
            target,
            initial_pos: popup_position(&ctx, response.rect),
            hint: hint.to_owned(),
            original: value.clone(),
            draft: value.clone(),
            replace: true,
            system_keyboard: false,
            request_native_focus: false,
        });
        if let Some(active) = state.as_ref() {
            ctx.data_mut(|data| data.insert_temp(pad_id(), active.clone()));
        }
        // Buttons do not start an IME session.
        ctx.memory_mut(|memory| memory.surrender_focus(target));
    }

    let mut accept = false;
    if let Some(mut active) = state.filter(|state| state.target == target && !state.system_keyboard) {
        if let Some(action) = draw_pad(&ctx, &active) {
            match action {
                Key::Accept => {
                    if *value != active.draft {
                        *value = active.draft;
                        response.mark_changed();
                    }
                    accept = true;
                    set_confirm(&ctx);
                    ctx.data_mut(|data| data.remove::<Active>(pad_id()));
                }
                Key::Cancel => {
                    if *value != active.original {
                        *value = active.original;
                        response.mark_changed();
                    }
                    ctx.data_mut(|data| data.remove::<Active>(pad_id()));
                }
                Key::Native => {
                    active.system_keyboard = true;
                    active.request_native_focus = true;
                    *value = active.draft.clone();
                    ctx.data_mut(|data| data.insert_temp(pad_id(), active));
                }
                other => {
                    apply_key(&mut active, other);
                    if *value != active.draft {
                        *value = active.draft.clone();
                        response.mark_changed();
                    }
                    ctx.data_mut(|data| data.insert_temp(pad_id(), active));
                }
            }
        } else {
            // A live preview can update the field from a manipulator; do not
            // overwrite it until the user actually edits the keypad.
            if !active.replace && *value != active.draft {
                *value = active.draft.clone();
                response.mark_changed();
            }
            ctx.data_mut(|data| data.insert_temp(pad_id(), active));
        }
    }
    (response, accept)
}
