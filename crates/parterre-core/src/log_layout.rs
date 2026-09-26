//! The log window's layouts: four fixed arrangements of its three panes (commits, details,
//! changed files), and where the two dividers of each sit. Decided in #29; the prototype is on
//! the branch `prototype/log-window`. No docking, no hiding panes, no custom layouts: a new
//! layout is a deliberate addition to [`LogLayout::ALL`].

use serde::{Deserialize, Serialize};

/// One of the fixed arrangements of the log window's panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LogLayout {
    /// A: commits, details and changed files from top to bottom, as in TortoiseGit.
    #[default]
    Stacked,
    /// B: commits on the left; details above the changed files on the right.
    SideBySide,
    /// C: commits full width on top; details and changed files side by side below them.
    DetailsBelow,
    /// D: commits top left with the details under them; the changed files in a full-height
    /// column on the right.
    FilesRight,
}

impl LogLayout {
    pub const ALL: [LogLayout; 4] = [
        LogLayout::Stacked,
        LogLayout::SideBySide,
        LogLayout::DetailsBelow,
        LogLayout::FilesRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogLayout::Stacked => "Stacked",
            LogLayout::SideBySide => "Side by side",
            LogLayout::DetailsBelow => "Details and files below",
            LogLayout::FilesRight => "Files on the right",
        }
    }
}

/// Where the dividers between the panes are, per layout. Each layout has two, stored as
/// fractions of the room the panes share (the window body less the dividers themselves), so
/// they keep their proportions when the window is resized.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dividers {
    /// A: the commit list's and the details' shares of the height; the changed files get the
    /// rest.
    pub stacked: [f32; 2],
    /// B: the commit list's share of the width; the details' share of the right column's
    /// height.
    pub side_by_side: [f32; 2],
    /// C: the commit list's share of the height; the details' share of the width below it.
    pub details_below: [f32; 2],
    /// D: the left column's share of the width; the commit list's share of its height.
    pub files_right: [f32; 2],
}

impl Default for Dividers {
    /// As in the prototype.
    fn default() -> Self {
        Dividers {
            stacked: [0.45, 0.22],
            side_by_side: [0.56, 0.38],
            details_below: [0.55, 0.40],
            files_right: [0.58, 0.62],
        }
    }
}

impl Dividers {
    /// The smallest share of the height a pane gets.
    pub const MIN_HEIGHT: f32 = 0.08;
    /// The smallest share of the width a pane gets: the panes hold tables, which need more
    /// room across than down.
    pub const MIN_WIDTH: f32 = 0.15;

    /// The two fractions of `layout`.
    pub fn of(&self, layout: LogLayout) -> [f32; 2] {
        *self.slot(layout)
    }

    fn slot(&self, layout: LogLayout) -> &[f32; 2] {
        match layout {
            LogLayout::Stacked => &self.stacked,
            LogLayout::SideBySide => &self.side_by_side,
            LogLayout::DetailsBelow => &self.details_below,
            LogLayout::FilesRight => &self.files_right,
        }
    }

    fn slot_mut(&mut self, layout: LogLayout) -> &mut [f32; 2] {
        match layout {
            LogLayout::Stacked => &mut self.stacked,
            LogLayout::SideBySide => &mut self.side_by_side,
            LogLayout::DetailsBelow => &mut self.details_below,
            LogLayout::FilesRight => &mut self.files_right,
        }
    }

    /// Whether `layout`'s dividers are where they start.
    pub fn is_default(&self, layout: LogLayout) -> bool {
        self.of(layout) == Dividers::default().of(layout)
    }

    /// Puts `layout`'s dividers back where they start; the other layouts keep theirs.
    pub fn reset(&mut self, layout: LogLayout) {
        *self.slot_mut(layout) = Dividers::default().of(layout);
    }

    /// Whether divider `which` (0 or 1) of `layout` splits a width, i.e. is a vertical bar
    /// dragged sideways. The others split a height.
    pub fn splits_width(layout: LogLayout, which: usize) -> bool {
        matches!(
            (layout, which),
            (LogLayout::SideBySide, 0) | (LogLayout::DetailsBelow, 1) | (LogLayout::FilesRight, 0)
        )
    }

    /// Moves divider `which` (0 or 1) of `layout` to `at`: where its middle is, as a fraction
    /// of the room it divides. Kept where every pane gets at least its minimum share.
    ///
    /// In the stacked layout both dividers divide the same height, and `at` of the second is
    /// measured from the top too; each moves only the boundary between its own two panes.
    pub fn set(&mut self, layout: LogLayout, which: usize, at: f32) {
        if !at.is_finite() {
            return;
        }
        let d = self.slot_mut(layout);
        if layout == LogLayout::Stacked {
            let min = Dividers::MIN_HEIGHT;
            if which == 0 {
                // The boundary between details and files stays.
                let both = d[0] + d[1];
                d[0] = at.clamp(min, (both - min).max(min));
                d[1] = both - d[0];
            } else {
                d[1] = (at - d[0]).clamp(min, (1.0 - d[0] - min).max(min));
            }
            return;
        }
        let min = if Dividers::splits_width(layout, which) {
            Dividers::MIN_WIDTH
        } else {
            Dividers::MIN_HEIGHT
        };
        d[which.min(1)] = at.clamp(min, 1.0 - min);
    }

    /// The same dividers, each moved where [`Dividers::set`] would allow; values that aren't
    /// numbers go back to their defaults. For settings saved by hand or by another version.
    pub fn clamped(self) -> Dividers {
        let defaults = Dividers::default();
        let mut out = self;
        for layout in LogLayout::ALL {
            let saved = self.of(layout);
            if saved.iter().any(|v| !v.is_finite()) {
                out.reset(layout);
                continue;
            }
            *out.slot_mut(layout) = defaults.of(layout);
            if layout == LogLayout::Stacked {
                // The first from the top, then the second below it.
                let min = Dividers::MIN_HEIGHT;
                let first = saved[0].clamp(min, 1.0 - 2.0 * min);
                out.stacked = [first, saved[1].clamp(min, (1.0 - first - min).max(min))];
            } else {
                out.set(layout, 0, saved[0]);
                out.set(layout, 1, saved[1]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_pane_has_room(d: &Dividers) -> bool {
        let eps = 1e-5;
        let [a, b] = d.stacked;
        let stacked = a >= Dividers::MIN_HEIGHT - eps
            && b >= Dividers::MIN_HEIGHT - eps
            && 1.0 - a - b >= Dividers::MIN_HEIGHT - eps;
        let others = [
            LogLayout::SideBySide,
            LogLayout::DetailsBelow,
            LogLayout::FilesRight,
        ]
        .into_iter()
        .all(|layout| {
            (0..2).all(|which| {
                let min = if Dividers::splits_width(layout, which) {
                    Dividers::MIN_WIDTH
                } else {
                    Dividers::MIN_HEIGHT
                };
                let v = d.of(layout)[which];
                v >= min - eps && 1.0 - v >= min - eps
            })
        });
        stacked && others
    }

    #[test]
    fn defaults_leave_room_for_every_pane() {
        let d = Dividers::default();
        assert!(every_pane_has_room(&d));
        assert_eq!(d.clamped(), d);
        assert!(LogLayout::ALL.iter().all(|&l| d.is_default(l)));
        assert_eq!(LogLayout::default(), LogLayout::Stacked);
    }

    #[test]
    fn dragging_stops_at_the_minimum_share() {
        let mut d = Dividers::default();
        for layout in LogLayout::ALL {
            for which in 0..2 {
                for at in [-5.0, 0.0, 0.01, 0.5, 0.99, 1.0, 7.0] {
                    d.set(layout, which, at);
                    assert!(every_pane_has_room(&d), "{layout:?} {which} {at}: {d:?}");
                }
            }
        }
    }

    #[test]
    fn stacked_dividers_move_only_their_own_boundary() {
        let mut d = Dividers::default();
        // The first divider up: the commits shrink, the details grow, the files stay.
        d.set(LogLayout::Stacked, 0, 0.30);
        assert!((d.stacked[0] - 0.30).abs() < 1e-6);
        assert!((d.stacked[0] + d.stacked[1] - 0.67).abs() < 1e-6);
        // The second divider down: the details grow into the files, the commits stay.
        d.set(LogLayout::Stacked, 1, 0.80);
        assert!((d.stacked[0] - 0.30).abs() < 1e-6);
        assert!((d.stacked[1] - 0.50).abs() < 1e-6);
        // The first divider can't pass the second.
        d.set(LogLayout::Stacked, 0, 0.95);
        assert!((d.stacked[1] - Dividers::MIN_HEIGHT).abs() < 1e-6);
        assert!((d.stacked[0] + d.stacked[1] - 0.80).abs() < 1e-6);
    }

    #[test]
    fn reset_touches_only_the_current_layout() {
        let mut d = Dividers::default();
        d.set(LogLayout::SideBySide, 0, 0.3);
        d.set(LogLayout::FilesRight, 1, 0.3);
        assert!(!d.is_default(LogLayout::SideBySide));
        d.reset(LogLayout::SideBySide);
        assert!(d.is_default(LogLayout::SideBySide));
        assert!(!d.is_default(LogLayout::FilesRight));
    }

    #[test]
    fn clamping_repairs_bad_saved_values() {
        let d = Dividers {
            stacked: [0.9, 0.5],
            side_by_side: [f32::NAN, 0.5],
            details_below: [-1.0, 2.0],
            files_right: [0.5, 0.5],
        }
        .clamped();
        assert!(every_pane_has_room(&d));
        assert_eq!(d.side_by_side, Dividers::default().side_by_side);
        assert_eq!(d.files_right, [0.5, 0.5]);
        assert_eq!(
            d.details_below,
            [Dividers::MIN_HEIGHT, 1.0 - Dividers::MIN_WIDTH]
        );
    }

    #[test]
    fn a_bad_position_is_ignored() {
        let mut d = Dividers::default();
        d.set(LogLayout::DetailsBelow, 0, f32::NAN);
        assert_eq!(d, Dividers::default());
    }
}
