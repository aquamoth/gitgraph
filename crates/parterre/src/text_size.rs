//! The text size: egui's zoom factor, one for every window, remembered with the settings.
//! Ctrl+wheel and pinch change it anywhere but over the graph, and Ctrl+plus, minus and 0 in
//! the windows besides the main one. The graph keeps its own zoom (see [`crate::view::View`]).

use eframe::egui::{self, Key, Modifiers};

/// The sizes Ctrl+plus and minus step through, as in browsers.
pub const STEPS: [f32; 13] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];
pub const MIN: f32 = STEPS[0];
pub const MAX: f32 = STEPS[STEPS.len() - 1];

/// The next step up from `size`.
pub fn larger(size: f32) -> f32 {
    STEPS.into_iter().find(|&s| s > size + 0.001).unwrap_or(MAX)
}

/// The next step down from `size`.
pub fn smaller(size: f32) -> f32 {
    STEPS
        .into_iter()
        .rev()
        .find(|&s| s < size - 0.001)
        .unwrap_or(MIN)
}

/// A size as saved, or given on the command line, within the steps' range.
pub fn sanitize(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(MIN, MAX)
    } else {
        1.0
    }
}

/// Turns Ctrl+wheel and pinch into steps, however egui spreads a wheel notch over frames: a
/// notch is one step, and so is pinching by a fifth.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wheel {
    /// The zoom gathered towards the next step, as its logarithm.
    pending: f32,
}

impl Wheel {
    /// Adds one frame's zoom (egui's `zoom_delta`); returns how many steps to go up (positive)
    /// or down (negative).
    pub fn steps(&mut self, delta: f32) -> i32 {
        // A gesture's remainder is dropped when it ends.
        if delta == 1.0 {
            self.pending = 0.0;
            return 0;
        }
        let step = 1.2_f32.ln();
        self.pending += delta.ln();
        let steps = (self.pending / step).trunc();
        self.pending -= steps * step;
        steps as i32
    }
}

/// Reads the text size input of the window `ui` is in into `size`: Ctrl+wheel and pinch, and
/// with `keys`, Ctrl+plus, minus and 0. Call it once per window and frame, and not where the
/// graph takes the wheel.
pub fn read_input(ui: &egui::Ui, size: &mut f32, keys: bool) {
    let delta = ui.input(|i| i.zoom_delta());
    let id = egui::Id::new(("text-size-wheel", ui.ctx().viewport_id()));
    let steps = ui.data_mut(|d| d.get_temp_mut_or_default::<Wheel>(id).steps(delta));
    for _ in 0..steps {
        *size = larger(*size);
    }
    for _ in steps..0 {
        *size = smaller(*size);
    }
    if keys {
        ui.input_mut(|i| {
            let mut command = |k: Key| i.consume_key(Modifiers::COMMAND, k);
            if command(Key::Plus) || command(Key::Equals) {
                *size = larger(*size);
            }
            if command(Key::Minus) {
                *size = smaller(*size);
            }
            if command(Key::Num0) {
                *size = 1.0;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_up_and_down() {
        assert_eq!(larger(1.0), 1.1);
        assert_eq!(smaller(1.0), 0.9);
        assert_eq!(larger(1.25), 1.5);
        assert_eq!(smaller(1.25), 1.1);
    }

    #[test]
    fn stops_at_the_ends() {
        assert_eq!(larger(MAX), MAX);
        assert_eq!(smaller(MIN), MIN);
    }

    #[test]
    fn a_size_between_steps_goes_to_the_neighbouring_steps() {
        assert_eq!(larger(1.2), 1.25);
        assert_eq!(smaller(1.2), 1.1);
    }

    #[test]
    fn saved_sizes_are_kept_within_range() {
        assert_eq!(sanitize(1.2), 1.2);
        assert_eq!(sanitize(0.1), MIN);
        assert_eq!(sanitize(9.0), MAX);
        assert_eq!(sanitize(f32::NAN), 1.0);
        assert_eq!(sanitize(f32::INFINITY), 1.0);
    }

    /// A wheel notch as egui smooths it: exp(50 / 200) in all, over several frames.
    fn notch(wheel: &mut Wheel, sign: f32) -> i32 {
        let total = sign * 0.25;
        let parts = [0.4, 0.3, 0.2, 0.1];
        let mut steps = 0;
        for part in parts {
            steps += wheel.steps((total * part).exp());
        }
        steps + wheel.steps(1.0)
    }

    #[test]
    fn a_wheel_notch_is_one_step() {
        let mut wheel = Wheel::default();
        assert_eq!(notch(&mut wheel, 1.0), 1);
        assert_eq!(notch(&mut wheel, 1.0), 1);
        assert_eq!(notch(&mut wheel, -1.0), -1);
    }

    #[test]
    fn a_small_pinch_does_nothing_and_is_forgotten_when_it_ends() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.steps(1.1), 0);
        assert_eq!(wheel.steps(1.0), 0);
        assert_eq!(wheel.steps(1.1), 0);
    }

    #[test]
    fn a_long_pinch_steps_again_and_again() {
        let mut wheel = Wheel::default();
        let steps: i32 = (0..20).map(|_| wheel.steps(1.05)).sum();
        // 1.05^20 = 2.65, about five fifths.
        assert_eq!(steps, 5);
        assert_eq!(wheel.steps(0.5), -3);
    }
}
