// SPDX-License-Identifier: GPL-3.0-only

//! The xterm mouse-tracking protocol (gpui-terminal Task 3): the
//! encoding moved here, framework-free, from the libcosmic
//! frontend's `mouse_reporter.rs` (itself adapted from cosmic-term
//! at commit 9129277) so both frontends consume one implementation
//! instead of carrying copies. The toolkit halves — turning iced or
//! GPUI pointer events into [`PointerEvent`] — stay with their
//! frontends; everything past that translation is this module's.

use super::Terminal;

/// The pointer buttons the xterm encodings number (0/1/2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}

/// One pointer occurrence in the toolkit-neutral shape: a press, a
/// release, or a motion. Motion carries no coordinates — the caller
/// supplies the cell it landed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEvent {
    Pressed(PointerButton),
    Released(PointerButton),
    Moved,
}

/// The modifiers the encodings fold into the button number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

/// A wheel delta in the toolkit-neutral shape: whole lines (one per
/// notch, or a fraction of one on high-resolution wheels) or raw
/// pixels the accumulation divides by the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerScrollDelta {
    Lines { x: f32, y: f32 },
    Pixels { x: f32, y: f32 },
}

/// Builds xterm mouse-tracking escape sequences from pointer
/// events, in the legacy/UTF-8 and SGR encodings.
#[derive(Default)]
pub struct MouseReporter {
    last_movment_x: Option<u32>,
    last_movment_y: Option<u32>,
    accumulated_scroll_x: f32,
    accumulated_scroll_y: f32,
    button: Option<PointerButton>,
}

impl MouseReporter {
    /// Accumulate a scroll delta into whole cell lines (x, y),
    /// keeping the fractional remainder for the next event.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "trunc splits accumulated cell lines from the fractional remainder; line counts < 2^24"
    )]
    pub fn accumulate_scroll(
        &mut self,
        delta: PointerScrollDelta,
        cell_width: f32,
        cell_height: f32,
    ) -> (i32, i32) {
        match delta {
            PointerScrollDelta::Lines { x, y } => {
                self.accumulated_scroll_x += x;
                self.accumulated_scroll_y += y;
            }
            PointerScrollDelta::Pixels { x, y } => {
                self.accumulated_scroll_x += x / cell_width;
                self.accumulated_scroll_y += y / cell_height;
            }
        }

        let lines_x = self.accumulated_scroll_x as i32;
        let lines_y = self.accumulated_scroll_y as i32;
        self.accumulated_scroll_x -= lines_x as f32;
        self.accumulated_scroll_y -= lines_y as f32;
        (lines_x, lines_y)
    }

    fn button_number(button: PointerButton) -> u8 {
        match button {
            PointerButton::Left => 0,
            PointerButton::Middle => 1,
            PointerButton::Right => 2,
        }
    }

    // Implemented according to
    // <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Mouse-Tracking>
    /// The legacy (X10) mouse encoding, UTF-8-extended when
    /// `is_utf8`; `None` when the event cannot be encoded (no
    /// button, or a position beyond the encoding's range).
    pub fn normal_mouse_code(
        &mut self,
        event: &PointerEvent,
        modifiers: PointerModifiers,
        is_utf8: bool,
        x: u32,
        y: u32,
    ) -> Option<Vec<u8>> {
        // Buttons are handled slightly differently between normal and
        // SGR: for normal/utf8 the button release is always reported
        // as button 3.
        let mut button = (match event {
            PointerEvent::Pressed(b) => {
                self.button = Some(*b);
                Some(Self::button_number(*b))
            }
            PointerEvent::Released(_) => {
                self.button = None;
                Some(3)
            }
            PointerEvent::Moved => {
                // A held button's motion is reported as button 32 +
                // 0,1,2 and event code M; only if a button is
                // previously pressed. On button-motion events, xterm
                // adds 32 to the event code (the third character,
                // Cb) — motion into cell x,y with button 1 down is
                // reported as CSI M @ CxCy (@ = 32 + 0 + 32).
                if (self.last_movment_x, self.last_movment_y) == (Some(x), Some(y)) {
                    return None;
                }
                self.last_movment_x = Some(x);
                self.last_movment_y = Some(y);
                self.button.map(|b| Self::button_number(b) + 32)
            }
        })?;

        if modifiers.shift {
            button += 4;
        }
        if modifiers.alt {
            button += 8;
        }
        if modifiers.control {
            button += 16;
        }

        // Normal mode has a max of 223 (255 - 32), while utf8
        // extends this to 2015.
        let max_point: usize = if is_utf8 { 2015 } else { 223 };
        if x as usize >= max_point || y as usize >= max_point {
            return None;
        }

        let utf8_encode_and_append = |mut pos: u32, dest: &mut Vec<u8>| {
            pos += 1 + 32;
            let mut utf8 = [0; 2]; // Large enough since we have a max of 2015
            dest.extend_from_slice(
                (char::from_u32(pos).unwrap()) // Safe due to our specific range: pos max 2047
                    .encode_utf8(&mut utf8)
                    .as_bytes(),
            );
        };

        // xterm ctlseqs: Likewise, Cb will be UTF-8 encoded, to
        // reduce confusion with wheel mouse events. Always, or only
        // when the coordinates are used? No other terminal seems to
        // do this at all? Doing what they are doing for now.
        let mut buf: Vec<u8> = vec![b'\x1b', b'[', b'M', 32 + button];

        // For utf8, xterm ctlseqs say: for positions less than 95,
        // the resulting output is identical under both modes.
        if is_utf8 && x >= 95 {
            utf8_encode_and_append(x, &mut buf);
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "one-byte xterm encoding: x < 95 here, or < 223 in legacy mode; 33 + x fits u8 exactly"
            )]
            buf.push(32 + 1 + x as u8);
        }

        if is_utf8 && y >= 95 {
            utf8_encode_and_append(y, &mut buf);
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "one-byte xterm encoding: y < 95 here, or < 223 in legacy mode; 33 + y fits u8 exactly"
            )]
            buf.push(32 + 1 + y as u8);
        }
        Some(buf)
    }

    /// The SGR (mode 1006) mouse encoding for press/release/motion
    /// events.
    pub fn sgr_mouse_code(
        &mut self,
        event: &PointerEvent,
        modifiers: PointerModifiers,
        x: u32,
        y: u32,
    ) -> Option<Vec<u8>> {
        let (button_no, event_code) = (match event {
            PointerEvent::Pressed(button) => {
                // A press is reported as button 0,1,2 and event code M.
                self.button = Some(*button);
                Some((Some(Self::button_number(*button)), "M"))
            }
            PointerEvent::Released(button) => {
                // A release is reported as button 0,1,2 and event code m.
                self.button = None;
                Some((Some(Self::button_number(*button)), "m"))
            }
            PointerEvent::Moved => {
                // A held button's motion is reported as button 32 +
                // 0,1,2 and event code M, and only if a button is
                // previously pressed.
                if (self.last_movment_x, self.last_movment_y) == (Some(x), Some(y)) {
                    return None;
                }
                self.last_movment_x = Some(x);
                self.last_movment_y = Some(y);
                self.button
                    .map(|button| (Some(Self::button_number(button) + 32), "M"))
            }
        })?;

        if let Some(mut button_no) = button_no {
            if modifiers.shift {
                button_no += 4;
            }
            if modifiers.alt {
                button_no += 8;
            }
            if modifiers.control {
                button_no += 16;
            }
            let term_code = format!("\x1b[<{};{};{}{}", button_no, x + 1, y + 1, event_code);
            Some(term_code.as_bytes().to_vec())
        } else {
            None
        }
    }

    /// SGR wheel-scroll events, one per accumulated cell line.
    #[allow(
        clippy::too_many_arguments,
        reason = "the xterm SGR encoding needs all six inputs; a param struct would obscure the upstream shape"
    )]
    pub fn sgr_mouse_wheel_scroll(
        &mut self,
        term_cell_width: f32,
        term_cell_height: f32,
        delta: PointerScrollDelta,
        modifiers: PointerModifiers,
        x: u32,
        y: u32,
    ) -> impl Iterator<Item = Vec<u8>> {
        let (lines_x, lines_y) = self.accumulate_scroll(delta, term_cell_width, term_cell_height);

        // Resolve modifier flags
        let mut modifier_flags = 0;
        if modifiers.shift {
            modifier_flags += 4;
        }
        if modifiers.alt {
            modifier_flags += 8;
        }
        if modifiers.control {
            modifier_flags += 16;
        }

        // Resolve base inputs
        let button_no_y = match lines_y.cmp(&0) {
            std::cmp::Ordering::Less => 65,    // Wheel Down
            std::cmp::Ordering::Greater => 64, // Wheel Up
            std::cmp::Ordering::Equal => 0,    // Unused
        };

        let button_no_x = match lines_x.cmp(&0) {
            std::cmp::Ordering::Less => 66,    // Wheel Left
            std::cmp::Ordering::Greater => 67, // Wheel Right
            std::cmp::Ordering::Equal => 0,    // Unused
        };

        // Generate term codes
        let x_iter = std::iter::repeat_n(button_no_x, lines_x.unsigned_abs() as usize);
        let y_iter = std::iter::repeat_n(button_no_y, lines_y.unsigned_abs() as usize);

        x_iter
            .chain(y_iter)
            .map(move |button_no| button_no + modifier_flags)
            .map(move |button_no| {
                let term_code = format!("\x1b[<{};{};{}M", button_no, x + 1, y + 1);
                term_code.as_bytes().to_vec()
            })
    }
}

/// Forward one pointer event to the pty as xterm tracking bytes when
/// the program asked for them.
pub fn report_mouse(
    terminal: &Terminal,
    reporter: &mut MouseReporter,
    event: &PointerEvent,
    modifiers: PointerModifiers,
    x: u32,
    y: u32,
) {
    let term_lock = terminal.term.lock();
    let mode = term_lock.mode();

    #[expect(
        clippy::collapsible_else_if,
        reason = "the SGR/normal split stays parallel if/else blocks, mirroring upstream's shape"
    )]
    if mode.contains(alacritty_terminal::term::TermMode::SGR_MOUSE) {
        if let Some(code) = reporter.sgr_mouse_code(event, modifiers, x, y) {
            terminal.input_no_scroll(code);
        }
    } else {
        if let Some(code) = reporter.normal_mouse_code(
            event,
            modifiers,
            mode.contains(alacritty_terminal::term::TermMode::UTF8_MOUSE),
            x,
            y,
        ) {
            terminal.input_no_scroll(code);
        }
    }
}

/// Forward one wheel delta to the program when it tracks the mouse.
pub fn scroll_mouse(
    terminal: &Terminal,
    reporter: &mut MouseReporter,
    delta: PointerScrollDelta,
    modifiers: PointerModifiers,
    x: u32,
    y: u32,
) {
    let is_sgr = terminal
        .term
        .lock()
        .mode()
        .contains(alacritty_terminal::term::TermMode::SGR_MOUSE);

    if is_sgr {
        let codes = reporter.sgr_mouse_wheel_scroll(
            terminal.size().cell_width,
            terminal.size().cell_height,
            delta,
            modifiers,
            x,
            y,
        );

        for code in codes {
            terminal.input_no_scroll(code);
        }
    } else {
        scroll_as_arrows(terminal, reporter, delta);
    }
}

/// Scroll by arrow-key bytes — the shell's own scrollback.
pub fn scroll_as_arrows(
    terminal: &Terminal,
    reporter: &mut MouseReporter,
    delta: PointerScrollDelta,
) {
    const SCROLL_SPEED: u32 = 3;
    let cell_width = terminal.size().cell_width;
    let cell_height = terminal.size().cell_height;
    let (_, lines_y) = reporter.accumulate_scroll(delta, cell_width, cell_height);
    let is_app_cursor = terminal
        .term
        .lock()
        .mode()
        .contains(alacritty_terminal::term::TermMode::APP_CURSOR);
    let (up, down) = if is_app_cursor {
        (&b"\x1bOA"[..], &b"\x1bOB"[..])
    } else {
        (&b"\x1b[A"[..], &b"\x1b[B"[..])
    };
    for _ in 0..(lines_y.unsigned_abs() * SCROLL_SPEED) {
        if lines_y > 0 {
            terminal.input_no_scroll(up);
        } else if lines_y < 0 {
            terminal.input_no_scroll(down);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MouseReporter, PointerButton, PointerEvent, PointerModifiers, PointerScrollDelta};

    fn mods(shift: bool, alt: bool, control: bool) -> PointerModifiers {
        PointerModifiers {
            shift,
            alt,
            control,
        }
    }

    #[test]
    fn the_normal_encoding_presses_releases_and_dedups_motion() {
        let mut r = MouseReporter::default();
        // Press left at cell (2, 3): ESC [ M, Cb = 32 + 0 = space,
        // Cx = 32+1+2 = '#', Cy = 32+1+3 = '$'.
        assert_eq!(
            r.normal_mouse_code(
                &PointerEvent::Pressed(PointerButton::Left),
                mods(false, false, false),
                false,
                2,
                3
            ),
            Some(b"\x1b[M #$".to_vec())
        );
        // Release always reports button 3: Cb = 32 + 3 = '#'.
        assert_eq!(
            r.normal_mouse_code(
                &PointerEvent::Released(PointerButton::Left),
                mods(false, false, false),
                false,
                2,
                3
            ),
            Some(b"\x1b[M##$".to_vec())
        );
        // Motion with no button held encodes nothing.
        assert_eq!(
            r.normal_mouse_code(&PointerEvent::Moved, mods(false, false, false), false, 4, 4),
            None
        );
        // Motion with the button held: press, move (5, 5) — Cb =
        // 32 + 0 + 32 = '@'; the same cell again dedups to nothing.
        r.normal_mouse_code(
            &PointerEvent::Pressed(PointerButton::Left),
            mods(false, false, false),
            false,
            0,
            0,
        );
        assert_eq!(
            r.normal_mouse_code(&PointerEvent::Moved, mods(false, false, false), false, 5, 5),
            Some(b"\x1b[M@&&".to_vec())
        );
        assert_eq!(
            r.normal_mouse_code(&PointerEvent::Moved, mods(false, false, false), false, 5, 5),
            None,
            "the same cell twice is one motion report"
        );
    }

    #[test]
    fn the_normal_encoding_folds_modifiers_into_the_button() {
        let mut r = MouseReporter::default();
        // Shift adds 4: Cb = 32 + 0 + 4 = '$'.
        assert_eq!(
            r.normal_mouse_code(
                &PointerEvent::Pressed(PointerButton::Left),
                mods(true, false, false),
                false,
                0,
                0
            ),
            Some(b"\x1b[M$!!".to_vec())
        );
        // Shift+Alt+Control adds 4+8+16 = 28: Cb = 32 + 28 = '`'.
        let mut r = MouseReporter::default();
        assert_eq!(
            r.normal_mouse_code(
                &PointerEvent::Pressed(PointerButton::Left),
                mods(true, true, true),
                false,
                0,
                0
            ),
            Some(b"\x1b[M<!!".to_vec())
        );
    }

    #[test]
    fn the_normal_encoding_extends_coordinates_utf8_beyond_94() {
        let mut r = MouseReporter::default();
        // UTF-8 mode at x = 100: Cx = 100 + 33 = 133 → two UTF-8
        // bytes C2 85. Below 95 the encodings are identical, so the
        // y = 3 coordinate stays one byte.
        assert_eq!(
            r.normal_mouse_code(
                &PointerEvent::Pressed(PointerButton::Left),
                mods(false, false, false),
                true,
                100,
                3
            ),
            Some(b"\x1b[M \xC2\x85$".to_vec())
        );
        // Legacy mode refuses the position its one-byte form cannot
        // hold (x must stay under 223).
        let mut r = MouseReporter::default();
        assert_eq!(
            r.normal_mouse_code(
                &PointerEvent::Pressed(PointerButton::Left),
                mods(false, false, false),
                false,
                300,
                0
            ),
            None
        );
    }

    #[test]
    fn the_sgr_encoding_carries_button_coordinates_and_motion() {
        let mut r = MouseReporter::default();
        // Press left at (0, 0): coordinates are 1-based in the wire
        // form.
        assert_eq!(
            r.sgr_mouse_code(
                &PointerEvent::Pressed(PointerButton::Left),
                mods(false, false, false),
                0,
                0
            ),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        // Middle press, then motion with it held: 1 + 32 = 33.
        assert_eq!(
            r.sgr_mouse_code(
                &PointerEvent::Pressed(PointerButton::Middle),
                mods(false, false, false),
                0,
                0
            ),
            Some(b"\x1b[<1;1;1M".to_vec())
        );
        assert_eq!(
            r.sgr_mouse_code(&PointerEvent::Moved, mods(false, false, false), 1, 2),
            Some(b"\x1b[<33;2;3M".to_vec())
        );
        // Release reports the same button number with lowercase m.
        assert_eq!(
            r.sgr_mouse_code(
                &PointerEvent::Released(PointerButton::Middle),
                mods(false, false, false),
                1,
                2
            ),
            Some(b"\x1b[<1;2;3m".to_vec())
        );
        // Motion without a button encodes nothing, and modifiers
        // fold into the button number as in normal mode.
        assert_eq!(
            r.sgr_mouse_code(&PointerEvent::Moved, mods(false, false, false), 1, 2),
            None
        );
        assert_eq!(
            r.sgr_mouse_code(
                &PointerEvent::Pressed(PointerButton::Right),
                mods(false, true, false),
                0,
                0
            ),
            Some(b"\x1b[<10;1;1M".to_vec())
        );
    }

    #[test]
    fn the_sgr_wheel_reports_one_code_per_line_with_modifiers() {
        let mut r = MouseReporter::default();
        // Wheel up (y = +1) at (4, 5): button 64, one code.
        let codes: Vec<Vec<u8>> = r
            .sgr_mouse_wheel_scroll(
                8.,
                20.,
                PointerScrollDelta::Lines { x: 0., y: 1. },
                mods(false, false, false),
                4,
                5,
            )
            .collect();
        assert_eq!(codes, vec![b"\x1b[<64;5;6M".to_vec()]);
        // Wheel down two lines with Ctrl: button 65 + 16 = 81, twice.
        let codes: Vec<Vec<u8>> = r
            .sgr_mouse_wheel_scroll(
                8.,
                20.,
                PointerScrollDelta::Lines { x: 0., y: -2. },
                mods(false, false, true),
                4,
                5,
            )
            .collect();
        assert_eq!(
            codes,
            vec![b"\x1b[<81;5;6M".to_vec(), b"\x1b[<81;5;6M".to_vec()]
        );
    }

    #[test]
    fn pixel_deltas_accumulate_across_events() {
        let mut r = MouseReporter::default();
        // Half a line per event: two events make one whole line.
        assert_eq!(
            r.accumulate_scroll(PointerScrollDelta::Pixels { x: 0., y: 10. }, 8., 20.),
            (0, 0)
        );
        assert_eq!(
            r.accumulate_scroll(PointerScrollDelta::Pixels { x: 0., y: 10. }, 8., 20.),
            (0, 1)
        );
        // Fractional line deltas accumulate the same way.
        assert_eq!(
            r.accumulate_scroll(PointerScrollDelta::Lines { x: 0., y: 0.25 }, 8., 20.),
            (0, 0)
        );
        assert_eq!(
            r.accumulate_scroll(PointerScrollDelta::Lines { x: 0., y: 0.25 }, 8., 20.),
            (0, 0)
        );
        assert_eq!(
            r.accumulate_scroll(PointerScrollDelta::Lines { x: 0., y: 0.25 }, 8., 20.),
            (0, 0)
        );
        assert_eq!(
            r.accumulate_scroll(PointerScrollDelta::Lines { x: 0., y: 0.25 }, 8., 20.),
            (0, 1)
        );
    }
}
