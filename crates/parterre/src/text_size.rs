//! Reading the text size input: egui's zoom factor, one for every window, remembered with the
//! settings. Ctrl+wheel and pinch change it anywhere but over the graph, and Ctrl+plus, minus
//! and 0 in the windows besides the main one. The graph keeps its own zoom (see
//! [`crate::view::View`]); the steps are in [`parterre_core::text_size`].

use eframe::egui::{self, Key, Modifiers};
use parterre_core::text_size::{Wheel, larger, smaller};

/// Reads the text size input of the window `ui` is in into `size`: Ctrl+wheel and pinch, and
/// with `keys`, Ctrl+plus, minus and 0. Call it once per window and frame, and not where the
/// graph takes the wheel.
pub fn read_input(ui: &egui::Ui, size: &mut f32, keys: bool) {
    let (delta, now) = ui.input(|i| (i.zoom_delta(), i.time));
    let id = egui::Id::new(("text-size-wheel", ui.ctx().viewport_id()));
    let steps = ui.data_mut(|d| d.get_temp_mut_or_default::<Wheel>(id).steps(delta, now));
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
