//! On-screen touch gamepad for mobile (Android) builds.
//!
//! The desktop games assume a keyboard. On a phone the only input
//! is the touch screen, so this module maps touches on translucent
//! on-screen buttons to the game's own key codes.
//!
//! Layout (in normalized viewport coordinates, 0..1):
//!
//!   bottom-left  : D-pad (up/down/left/right)  -> Up/Down/Left/Right
//!   bottom-right : A (confirm) -> Space, B (cancel) -> Escape
//!                  X (interact) -> F, Y (menu) -> Num1
//!
//! Multi-touch is supported: each finger is tracked independently so
//! the player can hold a direction while pressing a button.

use crate::input::{Key, KeyState};

/// A single on-screen button, defined by its centre and radius in
/// normalized window coordinates.
#[derive(Clone, Copy, Debug)]
pub struct TouchButton {
    pub key: Key,
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub label: &'static str,
}

impl TouchButton {
    const fn new(key: Key, cx: f32, cy: f32, radius: f32, label: &'static str) -> Self {
        Self {
            key,
            cx,
            cy,
            radius,
            label,
        }
    }

    /// Hit test in normalized coordinates.
    pub fn hit(&self, x: f32, y: f32) -> bool {
        let dx = x - self.cx;
        let dy = y - self.cy;
        dx * dx + dy * dy <= self.radius * self.radius
    }
}

/// The default button set. Radii are generous so the controls remain
/// usable on a phone held in landscape.
pub const BUTTONS: &[TouchButton] = &[
    // D-pad, bottom-left quadrant.
    TouchButton::new(Key::Up, 0.11, 0.72, 0.055, "^"),
    TouchButton::new(Key::Down, 0.11, 0.90, 0.055, "v"),
    TouchButton::new(Key::Left, 0.04, 0.81, 0.055, "<"),
    TouchButton::new(Key::Right, 0.18, 0.81, 0.055, ">"),
    // Face buttons, bottom-right quadrant.
    TouchButton::new(Key::Space, 0.90, 0.82, 0.062, "A"),
    TouchButton::new(Key::Escape, 0.80, 0.90, 0.055, "B"),
    TouchButton::new(Key::F, 0.80, 0.72, 0.055, "X"),
    TouchButton::new(Key::Num1, 0.96, 0.68, 0.045, "Y"),
];

/// Tracks which button each active touch id is currently pressing.
pub struct TouchGamepad {
    /// (touch id, button index) for every finger currently down.
    active: Vec<(u64, usize)>,
    /// Whether the gamepad layer is enabled.
    enabled: bool,
}

impl TouchGamepad {
    pub fn new() -> Self {
        Self {
            active: Vec::new(),
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.active.clear();
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Find the topmost button under a normalized point.
    fn button_at(&self, x: f32, y: f32) -> Option<usize> {
        BUTTONS.iter().position(|b| b.hit(x, y))
    }

    /// A finger landed at (x, y). Returns the key it presses, if any.
    pub fn touch_started(&mut self, id: u64, x: f32, y: f32) -> Option<Key> {
        if !self.enabled {
            return None;
        }
        let idx = self.button_at(x, y)?;
        // If this finger already holds a button, release it first so a
        // slide across the pad cannot leave a key stuck down.
        self.active.retain(|(tid, _)| *tid != id);
        self.active.push((id, idx));
        Some(BUTTONS[idx].key)
    }

    /// A finger moved. Handles sliding from one button to another.
    /// Returns (key released, key pressed) when the button changed.
    pub fn touch_moved(&mut self, id: u64, x: f32, y: f32) -> (Option<Key>, Option<Key>) {
        if !self.enabled {
            return (None, None);
        }
        let current = self
            .active
            .iter()
            .find(|(tid, _)| *tid == id)
            .map(|(_, i)| *i);
        let next = self.button_at(x, y);

        match (current, next) {
            (Some(cur), Some(nxt)) if cur == nxt => (None, None),
            (cur, next) => {
                self.active.retain(|(tid, _)| *tid != id);
                if let Some(nxt) = next {
                    self.active.push((id, nxt));
                }
                (cur.map(|i| BUTTONS[i].key), next.map(|i| BUTTONS[i].key))
            }
        }
    }

    /// A finger lifted or was cancelled. Returns the key it releases.
    pub fn touch_ended(&mut self, id: u64) -> Option<Key> {
        let idx = self
            .active
            .iter()
            .find(|(tid, _)| *tid == id)
            .map(|(_, i)| *i);
        self.active.retain(|(tid, _)| *tid != id);
        idx.map(|i| BUTTONS[i].key)
    }

    /// Apply held state of every active touch to the key buffer.
    /// Called once per event batch so held buttons stay down.
    pub fn apply_held(&self, states: &mut [KeyState]) {
        if !self.enabled {
            return;
        }
        for (_, idx) in &self.active {
            states[BUTTONS[*idx].key as usize].set_down(true);
        }
    }
}

impl Default for TouchGamepad {
    fn default() -> Self {
        Self::new()
    }
}
