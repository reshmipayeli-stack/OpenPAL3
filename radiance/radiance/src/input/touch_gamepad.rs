//! On-screen touch gamepad for Android.
//!
//! Phones have no keyboard, so this draws translucent controls over the
//! game and turns touches on them into the keys and stick axes the game
//! already reads:
//!
//! - Left stick (bottom-left): `LeftStickX` / `LeftStickY`, which drive
//!   character movement relative to the camera.
//! - A (bottom-right): `GamePadEast` + `F` + `Space` — interact, confirm,
//!   advance dialog.
//! - B: `Escape` + `GamePadSouth` — cancel, menu, skip movies.
//! - Camera buttons: `A` / `D` rotate the camera.
//!
//! Touches that start on a control are owned by it until the finger is
//! lifted, so they are not also forwarded to imgui as mouse clicks
//! (otherwise one tap on A would advance a dialog twice: once for the key,
//! once for the click). Touches elsewhere still reach imgui, so menus stay
//! tappable.
//!
//! The pad is only active on Android; on other platforms every function
//! here is a no-op.

use std::cell::RefCell;

use super::{Axis, AxisState, Key, KeyState};

fn enabled() -> bool {
    cfg!(target_os = "android")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Anchor {
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy)]
struct Button {
    /// Keys pressed together while the button is held.
    keys: &'static [Key],
    label: &'static str,
    anchor: Anchor,
    /// Centre offset from the anchored corner, in units of the shorter
    /// screen side.
    dx: f32,
    dy: f32,
    radius: f32,
}

const BUTTONS: &[Button] = &[
    Button {
        keys: &[Key::GamePadEast, Key::F, Key::Space],
        label: "A",
        anchor: Anchor::BottomRight,
        dx: 0.17,
        dy: 0.27,
        radius: 0.095,
    },
    Button {
        keys: &[Key::Escape, Key::GamePadSouth],
        label: "B",
        anchor: Anchor::BottomRight,
        dx: 0.38,
        dy: 0.13,
        radius: 0.08,
    },
    Button {
        keys: &[Key::A],
        label: "<",
        anchor: Anchor::BottomRight,
        dx: 0.40,
        dy: 0.44,
        radius: 0.065,
    },
    Button {
        keys: &[Key::D],
        label: ">",
        anchor: Anchor::BottomRight,
        dx: 0.15,
        dy: 0.52,
        radius: 0.065,
    },
];

/// Stick placement, same units as [`Button`].
const STICK_DX: f32 = 0.27;
const STICK_DY: f32 = 0.27;
/// Full deflection distance.
const STICK_RADIUS: f32 = 0.17;
/// A touch starting within this distance of the stick centre grabs it.
const STICK_GRAB_RADIUS: f32 = 0.27;

#[derive(Clone, Copy, PartialEq)]
enum Control {
    Stick,
    Button(usize),
}

struct Pad {
    width: f32,
    height: f32,
    /// Fingers currently owned by a control.
    active: Vec<(u64, Control)>,
    /// Fingers lifted since the last frame. They stay "owned" until the
    /// frame ends, so the imgui touch emulation sees them as ours no matter
    /// which window-event callback runs first.
    released: Vec<u64>,
    stick: [f32; 2],
    /// Set when the stick moved or was released, so the axis is written
    /// once more (to zero) instead of overriding a physical gamepad.
    stick_dirty: bool,
}

thread_local! {
    static PAD: RefCell<Pad> = const {
        RefCell::new(Pad {
            width: 0.,
            height: 0.,
            active: Vec::new(),
            released: Vec::new(),
            stick: [0., 0.],
            stick_dirty: false,
        })
    };
}

impl Pad {
    fn unit(&self) -> f32 {
        self.width.min(self.height)
    }

    fn corner_point(&self, anchor: Anchor, dx: f32, dy: f32) -> [f32; 2] {
        let u = self.unit();
        let x = match anchor {
            Anchor::BottomLeft => dx * u,
            Anchor::BottomRight => self.width - dx * u,
        };
        [x, self.height - dy * u]
    }

    fn stick_center(&self) -> [f32; 2] {
        self.corner_point(Anchor::BottomLeft, STICK_DX, STICK_DY)
    }

    fn button_center(&self, b: &Button) -> [f32; 2] {
        self.corner_point(b.anchor, b.dx, b.dy)
    }

    fn hit(&self, x: f32, y: f32) -> Option<Control> {
        if self.width <= 0. || self.height <= 0. {
            return None;
        }
        let u = self.unit();
        for (i, b) in BUTTONS.iter().enumerate() {
            let [cx, cy] = self.button_center(b);
            if dist2(x, y, cx, cy) <= (b.radius * u).powi(2) {
                return Some(Control::Button(i));
            }
        }
        let [cx, cy] = self.stick_center();
        if dist2(x, y, cx, cy) <= (STICK_GRAB_RADIUS * u).powi(2) {
            return Some(Control::Stick);
        }
        None
    }

    fn control_of(&self, id: u64) -> Option<Control> {
        self.active.iter().find(|(t, _)| *t == id).map(|(_, c)| *c)
    }

    fn set_stick_from(&mut self, x: f32, y: f32) {
        let [cx, cy] = self.stick_center();
        let r = STICK_RADIUS * self.unit();
        let mut sx = (x - cx) / r;
        // Screen y grows downward; LeftStickY > 0 means "forward".
        let mut sy = (cy - y) / r;
        let len = (sx * sx + sy * sy).sqrt();
        if len > 1. {
            sx /= len;
            sy /= len;
        }
        self.stick = [sx, sy];
        self.stick_dirty = true;
    }
}

fn dist2(x: f32, y: f32, cx: f32, cy: f32) -> f32 {
    (x - cx).powi(2) + (y - cy).powi(2)
}

fn press(states: &mut [KeyState], keys: &[Key]) {
    for k in keys {
        let s = &mut states[*k as usize];
        s.set_down(true);
        s.set_pressed(true);
    }
}

fn release(states: &mut [KeyState], keys: &[Key]) {
    for k in keys {
        let s = &mut states[*k as usize];
        s.set_down(false);
        s.set_released(true);
    }
}

/// Size of the drawable surface in physical pixels (the same space as
/// winit touch locations).
pub fn set_viewport(width: f32, height: f32) {
    if !enabled() {
        return;
    }
    PAD.with(|p| {
        let mut p = p.borrow_mut();
        p.width = width;
        p.height = height;
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
}

/// Feed one touch event. Writes key presses/releases into `states`.
pub fn handle_touch(states: &mut [KeyState], id: u64, phase: TouchPhase, x: f32, y: f32) {
    if !enabled() {
        return;
    }
    PAD.with(|p| {
        let mut p = p.borrow_mut();
        match phase {
            TouchPhase::Started => {
                if p.control_of(id).is_some() {
                    return;
                }
                match p.hit(x, y) {
                    Some(Control::Button(i)) => {
                        p.active.push((id, Control::Button(i)));
                        press(states, BUTTONS[i].keys);
                    }
                    Some(Control::Stick) => {
                        // Only one finger drives the stick.
                        if !p.active.iter().any(|(_, c)| *c == Control::Stick) {
                            p.active.push((id, Control::Stick));
                            p.set_stick_from(x, y);
                        }
                    }
                    None => {}
                }
            }
            TouchPhase::Moved => {
                if p.control_of(id) == Some(Control::Stick) {
                    p.set_stick_from(x, y);
                }
            }
            TouchPhase::Ended => {
                if let Some(c) = p.control_of(id) {
                    p.active.retain(|(t, _)| *t != id);
                    p.released.push(id);
                    match c {
                        Control::Button(i) => release(states, BUTTONS[i].keys),
                        Control::Stick => {
                            p.stick = [0., 0.];
                            p.stick_dirty = true;
                        }
                    }
                }
            }
        }
    });
}

/// True if this touch belongs to the pad and must not reach imgui.
pub fn owns_touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> bool {
    if !enabled() {
        return false;
    }
    PAD.with(|p| {
        let p = p.borrow();
        if p.control_of(id).is_some() || p.released.contains(&id) {
            return true;
        }
        phase == TouchPhase::Started && p.hit(x, y).is_some()
    })
}

/// Called once per input frame: publishes the stick to the axis state and
/// forgets fingers lifted during the previous frame.
pub fn end_frame(axis_states: &mut [AxisState]) {
    if !enabled() {
        return;
    }
    PAD.with(|p| {
        let mut p = p.borrow_mut();
        let stick_held = p.active.iter().any(|(_, c)| *c == Control::Stick);
        if stick_held || p.stick_dirty {
            axis_states[Axis::LeftStickX as usize].set_value(p.stick[0]);
            axis_states[Axis::LeftStickY as usize].set_value(p.stick[1]);
            p.stick_dirty = false;
        }
        p.released.clear();
    });
}

/// Draw the controls on top of everything else.
pub fn draw_overlay(ui: &imgui::Ui) {
    if !enabled() {
        return;
    }
    PAD.with(|p| {
        let p = p.borrow();
        if p.width <= 0. || p.height <= 0. {
            return;
        }
        let u = p.unit();
        let dl = ui.get_foreground_draw_list();
        let idle = [1.0, 1.0, 1.0, 0.22];
        let held = [1.0, 0.85, 0.35, 0.45];
        let ring = [1.0, 1.0, 1.0, 0.55];
        let text = [1.0, 1.0, 1.0, 0.9];

        // Stick: base ring + knob.
        let [cx, cy] = p.stick_center();
        let r = STICK_RADIUS * u;
        dl.add_circle([cx, cy], r, idle).filled(true).build();
        dl.add_circle([cx, cy], r, ring).thickness(2.0).build();
        let knob = [cx + p.stick[0] * r, cy - p.stick[1] * r];
        let knob_color = if p.active.iter().any(|(_, c)| *c == Control::Stick) {
            held
        } else {
            ring
        };
        dl.add_circle(knob, r * 0.45, knob_color).filled(true).build();

        for (i, b) in BUTTONS.iter().enumerate() {
            let [bx, by] = p.button_center(b);
            let br = b.radius * u;
            let pressed = p.active.iter().any(|(_, c)| *c == Control::Button(i));
            dl.add_circle([bx, by], br, if pressed { held } else { idle })
                .filled(true)
                .build();
            dl.add_circle([bx, by], br, ring).thickness(2.0).build();
            let size = ui.calc_text_size(b.label);
            dl.add_text([bx - size[0] / 2., by - size[1] / 2.], text, b.label);
        }
    });
}
