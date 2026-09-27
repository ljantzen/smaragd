//! Detects a double tap of the Shift key — IntelliJ's "Shift Shift" gesture for
//! Search Everywhere. Driven by `Event::ModifiersChanged`, which both eframe
//! backends (winit and web) emit on every modifier transition. egui 0.36 also
//! reports the Shift key itself as `Event::Key { key: ShiftLeft | ShiftRight }`
//! (at least through egui-winit); those are ignored rather than counted as
//! "another key", or every tap would disqualify itself.
//!
//! Pure state machine: `app.rs` feeds it the frame's events in order, so it's
//! unit-testable without an `egui::Context`.

/// Max time, in seconds, between the first tap's release and the second's.
pub const DOUBLE_TAP_WINDOW: f64 = 0.35;

/// One input observation, reduced to what the detector cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapInput {
    /// The modifier state changed: whether Shift is now down, and whether any
    /// *other* modifier (Ctrl/Alt/Cmd) is.
    Modifiers { shift: bool, others: bool },
    /// Any other key, text, paste, or pointer-button event — anything that means
    /// Shift is being used as a modifier (typing a capital, Shift+click) rather
    /// than tapped on its own.
    Other,
}

#[derive(Debug, Default)]
pub struct DoubleTapDetector {
    shift_down: bool,
    /// Set when something other than Shift happened while it was held, so its
    /// release doesn't count as a clean tap.
    dirty: bool,
    /// When the last clean tap was released, if it's still eligible to pair with
    /// the next one.
    last_tap: Option<f64>,
}

impl DoubleTapDetector {
    /// Feed one input at time `now` (seconds, e.g. `egui::InputState::time`).
    /// Returns `true` exactly on the release that completes a double tap.
    pub fn feed(&mut self, now: f64, input: TapInput) -> bool {
        match input {
            TapInput::Other => {
                self.dirty = true;
                self.last_tap = None;
                false
            }
            TapInput::Modifiers { shift, others } => {
                if others {
                    self.dirty = true;
                    self.last_tap = None;
                }
                let pressed = shift && !self.shift_down;
                let released = !shift && self.shift_down;
                self.shift_down = shift;
                if pressed {
                    self.dirty = others;
                    false
                } else if released {
                    let clean = !std::mem::replace(&mut self.dirty, false);
                    if !clean {
                        self.last_tap = None;
                        false
                    } else if self
                        .last_tap
                        .is_some_and(|first| now - first <= DOUBLE_TAP_WINDOW)
                    {
                        // Consumed, so a third tap starts a fresh pair rather
                        // than immediately triggering again.
                        self.last_tap = None;
                        true
                    } else {
                        self.last_tap = Some(now);
                        false
                    }
                } else {
                    false
                }
            }
        }
    }
}

/// Reduce an egui event to a `TapInput`, or `None` for events that shouldn't
/// affect the gesture (pointer movement, focus, scrolling, ...).
pub fn tap_input(event: &egui::Event) -> Option<TapInput> {
    match event {
        egui::Event::ModifiersChanged(m) => Some(TapInput::Modifiers {
            shift: m.shift,
            others: m.ctrl || m.alt || m.mac_cmd || m.command,
        }),
        egui::Event::Key {
            key: egui::Key::ShiftLeft | egui::Key::ShiftRight,
            ..
        } => None,
        egui::Event::Key { .. }
        | egui::Event::Text(_)
        | egui::Event::Paste(_)
        | egui::Event::PointerButton { .. } => Some(TapInput::Other),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWN: TapInput = TapInput::Modifiers {
        shift: true,
        others: false,
    };
    const UP: TapInput = TapInput::Modifiers {
        shift: false,
        others: false,
    };

    fn tap(detector: &mut DoubleTapDetector, at: f64) -> bool {
        let pressed = detector.feed(at, DOWN);
        let released = detector.feed(at + 0.05, UP);
        assert!(!pressed, "a press alone must never trigger");
        released
    }

    #[test]
    fn two_quick_clean_taps_trigger() {
        let mut detector = DoubleTapDetector::default();
        assert!(!tap(&mut detector, 0.0));
        assert!(tap(&mut detector, 0.2));
    }

    #[test]
    fn a_second_tap_after_the_window_does_not_trigger() {
        let mut detector = DoubleTapDetector::default();
        assert!(!tap(&mut detector, 0.0));
        assert!(!tap(&mut detector, 1.0));
        // ...but it does start a new pair.
        assert!(tap(&mut detector, 1.2));
    }

    #[test]
    fn typing_capitals_never_triggers() {
        let mut detector = DoubleTapDetector::default();
        for at in [0.0, 0.1, 0.2, 0.3] {
            detector.feed(at, DOWN);
            detector.feed(at + 0.01, TapInput::Other);
            assert!(!detector.feed(at + 0.02, UP));
        }
    }

    #[test]
    fn a_keypress_between_taps_cancels_the_pair() {
        let mut detector = DoubleTapDetector::default();
        assert!(!tap(&mut detector, 0.0));
        detector.feed(0.1, TapInput::Other);
        assert!(!tap(&mut detector, 0.15));
    }

    #[test]
    fn three_taps_trigger_only_once() {
        let mut detector = DoubleTapDetector::default();
        assert!(!tap(&mut detector, 0.0));
        assert!(tap(&mut detector, 0.1));
        assert!(!tap(&mut detector, 0.2));
    }

    fn shift_key(pressed: bool) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::ShiftLeft,
            physical_key: Some(egui::Key::ShiftLeft),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn the_shift_key_event_itself_is_ignored() {
        assert_eq!(tap_input(&shift_key(true)), None);
        assert_eq!(tap_input(&shift_key(false)), None);
    }

    #[test]
    fn other_keys_still_count_as_other_input() {
        let a = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        };
        assert_eq!(tap_input(&a), Some(TapInput::Other));
    }

    #[test]
    fn shift_with_another_modifier_is_not_a_tap() {
        let mut detector = DoubleTapDetector::default();
        let ctrl_shift = TapInput::Modifiers {
            shift: true,
            others: true,
        };
        detector.feed(0.0, ctrl_shift);
        detector.feed(0.05, UP);
        assert!(!tap(&mut detector, 0.1));
    }
}
