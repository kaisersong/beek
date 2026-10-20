//! Selection ownership and placement policy used by the macOS monitor.
//! A selection retained by an app is not evidence of a new selection gesture.

#[derive(Default)]
pub struct SelectionGate {
    context: Option<((i32, u64), u64)>,
    blocked_text: Option<String>,
    invalid_gesture_press: Option<u64>,
}

impl SelectionGate {
    pub fn gesture_allowed(&self, owner: (i32, u64), press: u64, gesture: bool) -> bool {
        gesture && self.context == Some((owner, press)) && self.invalid_gesture_press != Some(press)
    }

    pub fn allows(
        &mut self,
        owner: (i32, u64),
        press: u64,
        gesture: bool,
        text: Option<&str>,
    ) -> bool {
        if let Some((previous_owner, previous_press)) = self.context {
            if previous_owner != owner && previous_press == press {
                // A gesture cannot migrate into another app/window while its timer is live.
                self.invalid_gesture_press = Some(press);
            }
        }
        let gesture = gesture && self.invalid_gesture_press != Some(press);
        if self.context != Some((owner, press)) {
            self.context = Some((owner, press));
            self.blocked_text = text.map(str::to_owned);
            if !gesture {
                return false;
            }
        }
        if gesture {
            self.blocked_text = None;
            return text.is_some();
        }
        match text {
            // AX can disappear briefly while an app rebuilds its tree. That must not
            // erase the baseline and turn a recovered old selection into a new one.
            None => false,
            Some(text) => self.blocked_text.as_deref() != Some(text),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Anchor {
    Selection,
    Press(f64, f64),
    Pointer(f64, f64),
}

pub fn anchor(
    source: &str,
    origin: (f64, f64),
    press: Option<(f64, f64)>,
    pointer: Option<(f64, f64)>,
) -> Anchor {
    if matches!(
        source,
        "element" | "parent" | "tree" | "webarea" | "at-position" | "at-position-parent"
    ) {
        return Anchor::Selection;
    }
    let far = |p: (f64, f64)| (p.0 - origin.0).abs() >= 120. || (p.1 - origin.1).abs() >= 120.;
    if let Some(p) = press.filter(|p| far(*p)) {
        return Anchor::Press(p.0, p.1);
    }
    if let Some(p) = pointer.filter(|p| far(*p)) {
        return Anchor::Pointer(p.0, p.1);
    }
    Anchor::Selection
}

pub fn gesture_for_point(gesture: bool, point_owner: Option<i32>, foreground: i32) -> bool {
    gesture && point_owner.map(|pid| pid == foreground).unwrap_or(true)
}

pub fn double_click(elapsed_ms: u128, previous: (f64, f64), current: (f64, f64)) -> bool {
    elapsed_ms < 400 && (previous.0 - current.0).abs() <= 5. && (previous.1 - current.1).abs() <= 5.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_dock_click_and_app_switch_do_not_reuse_old_selection() {
        let mut gate = SelectionGate::default();
        assert!(gate.allows((10, 1), 1, true, Some("selected")));
        for _ in 0..20 {
            assert!(!gate.allows((10, 1), 2, false, Some("selected")));
        }
        for _ in 0..20 {
            assert!(!gate.allows((20, 1), 2, false, Some("retained in ChatGPT")));
        }
        assert!(!gate.allows((20, 2), 2, false, Some("retained in another window")));
        assert!(!gate.allows((20, 2), 2, false, Some("retained in another window")));
        // Foreground AX becomes temporarily unavailable and recovers.
        assert!(!gate.allows((20, 1), 3, false, Some("retained in ChatGPT")));
        for _ in 0..20 {
            assert!(!gate.allows((20, 1), 3, false, Some("retained in ChatGPT")));
        }
    }

    #[test]
    fn drag_double_click_and_keyboard_changes_still_work() {
        let mut gate = SelectionGate::default();
        assert!(!gate.allows((10, 1), 1, false, Some("old")));
        assert!(gate.allows((10, 1), 1, true, Some("old"))); // Same text selected again by double click.
        assert!(!gate.allows((10, 1), 2, false, Some("old")));
        assert!(gate.allows((10, 1), 2, false, Some("keyboard selection")));
        assert!(!gate.allows((10, 1), 2, false, None));
        assert!(gate.allows((10, 1), 2, false, Some("different keyboard selection")));
        assert!(!gate.allows((20, 1), 3, false, None));
        assert!(gate.allows((20, 1), 3, true, Some("dragged")));
    }

    #[test]
    fn a_recent_gesture_cannot_migrate_to_a_different_app_or_window() {
        let mut gate = SelectionGate::default();
        assert!(gate.allows((10, 1), 1, true, Some("selected")));
        assert!(!gate.allows((20, 1), 1, true, Some("other app retained text")));
        assert!(!gate.gesture_allowed((20, 1), 1, true)); // Copy fallbacks must also be blocked.
        assert!(!gate.allows((20, 1), 1, true, Some("other app retained text")));
        assert!(!gate.allows((10, 1), 1, true, Some("selected")));
        assert!(gate.allows((10, 1), 2, true, Some("selected again")));
        assert!(!gate.allows((10, 2), 2, true, Some("other window retained text")));
    }

    #[test]
    fn transient_ax_absence_does_not_revive_a_blocked_selection() {
        let mut gate = SelectionGate::default();
        assert!(!gate.allows((10, 1), 1, false, Some("retained")));
        assert!(!gate.allows((10, 1), 1, false, None));
        assert!(!gate.allows((10, 1), 1, false, Some("retained")));
        assert!(gate.allows((10, 1), 1, false, Some("new selection")));
        assert!(gate.allows((10, 1), 2, true, Some("retained")));
    }

    #[test]
    fn startup_does_not_popup_for_a_retained_selection() {
        let mut gate = SelectionGate::default();
        assert!(!gate.allows((10, 1), 0, false, Some("old")));
        assert!(!gate.allows((10, 1), 0, false, Some("old")));
        assert!(gate.allows((10, 1), 0, false, Some("new keyboard selection")));
    }

    #[test]
    fn trusted_selection_stays_at_its_bounds_even_when_click_is_in_dock() {
        for source in [
            "element",
            "parent",
            "tree",
            "webarea",
            "at-position",
            "at-position-parent",
        ] {
            assert_eq!(
                anchor(source, (200., 200.), Some((900., 900.)), Some((900., 900.))),
                Anchor::Selection
            );
        }
        assert_eq!(
            anchor("element-frame", (200., 200.), Some((900., 900.)), None),
            Anchor::Press(900., 900.)
        );
        assert_eq!(
            anchor("no-rect", (200., 200.), None, Some((900., 900.))),
            Anchor::Pointer(900., 900.)
        );
        assert_eq!(
            anchor("no-rect", (200., 200.), None, None),
            Anchor::Selection
        );
    }

    #[test]
    fn a_double_click_on_dock_cannot_trigger_app_copy_fallbacks() {
        assert!(!gesture_for_point(true, Some(99), 10));
        assert!(gesture_for_point(true, Some(10), 10));
        assert!(!gesture_for_point(false, Some(10), 10));
        // Some applications do not expose a hit-testable element; keep their existing fallback.
        assert!(gesture_for_point(true, None, 10));
    }

    #[test]
    fn fast_clicks_in_different_places_are_not_a_selection_double_click() {
        assert!(double_click(200, (200., 200.), (202., 201.)));
        assert!(!double_click(200, (200., 200.), (900., 900.)));
        assert!(!double_click(400, (200., 200.), (200., 200.)));
    }
}
