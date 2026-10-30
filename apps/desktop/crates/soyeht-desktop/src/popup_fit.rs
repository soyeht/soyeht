//! Where a popup menu opens and how tall it may be so that it fits the window: on the side of its
//! control that has the room for it, never taller than that room, and scrolling inside when its
//! list is longer. Pure geometry, so the rules are unit tests and not window behaviour.

use crate::theme::{desktop, space};

/// Which side of its control a popup opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// The control a popup belongs to, as far as the window's height is concerned: where its top and
/// its bottom edge are, in window points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub top: f32,
    pub bottom: f32,
}

/// What the popup is given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Want {
    /// The side the popup is drawn for.
    pub prefer: Side,
    /// Where the popup attaches on that side, as an offset from the control's top edge.
    pub offset: f32,
    /// How tall the popup is allowed to be when the window has the room.
    pub height: f32,
    /// The distance kept between the control and the popup when the popup changes sides.
    pub gap: f32,
}

/// Where the popup goes and how tall it may be there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub side: Side,
    /// The offset from the control's top edge at which the popup attaches.
    pub offset: f32,
    pub max_height: f32,
}

/// The room kept between a popup and the window's edge.
pub const MARGIN: f32 = space::X8;

/// The room a popup that attaches at `y` has on `side` in a window `window_height` tall.
fn room(side: Side, y: f32, window_height: f32) -> f32 {
    let room = match side {
        Side::Below => window_height - y - MARGIN,
        Side::Above => y - MARGIN,
    };
    room.max(0.0)
}

/// The side and the height for a popup of the control at `control`, or, while the control has
/// not been measured, for a popup that only has the window to go by. The preferred side is kept
/// while it has the room the popup asks for; otherwise the side with more room is used.
pub fn fit(want: Want, control: Option<Extent>, window_height: f32) -> Fit {
    let ceiling = want.height.min((window_height - 2.0 * MARGIN).max(0.0));
    let Some(control) = control else {
        return Fit {
            side: want.prefer,
            offset: want.offset,
            max_height: ceiling,
        };
    };
    let (other, other_offset) = match want.prefer {
        Side::Below => (Side::Above, -want.gap),
        Side::Above => (Side::Below, control.bottom - control.top + want.gap),
    };
    let preferred_room = room(want.prefer, control.top + want.offset, window_height);
    let other_room = room(other, control.top + other_offset, window_height);
    if preferred_room >= want.height || preferred_room >= other_room {
        Fit {
            side: want.prefer,
            offset: want.offset,
            max_height: want.height.min(preferred_room),
        }
    } else {
        Fit {
            side: other,
            offset: other_offset,
            max_height: want.height.min(other_room),
        }
    }
}

/// The distance kept between a control and a popup that moved to its other side.
pub const GAP: f32 = desktop::POPUP_GAP;

/// How tall a panel that hangs from `top` of the window may be, for one that is not placed by
/// [`fit`] (a fixed place in the window).
pub fn height_from(top: f32, window_height: f32) -> f32 {
    room(Side::Below, top, window_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: f32 = 628.0;

    fn bar_button() -> Extent {
        Extent {
            top: 8.0,
            bottom: 36.0,
        }
    }

    fn below(offset: f32, height: f32) -> Want {
        Want {
            prefer: Side::Below,
            offset,
            height,
            gap: 4.0,
        }
    }

    fn above(height: f32) -> Want {
        Want {
            prefer: Side::Above,
            offset: -4.0,
            height,
            gap: 4.0,
        }
    }

    #[test]
    fn a_short_menu_under_a_top_bar_button_keeps_its_height() {
        let fit = fit(below(32.0, 420.0), Some(bar_button()), WINDOW);
        assert_eq!(fit.side, Side::Below);
        assert_eq!(fit.offset, 32.0);
        assert_eq!(fit.max_height, 420.0);
    }

    #[test]
    fn a_tall_menu_is_cut_to_the_room_below_the_button() {
        let fit = fit(below(32.0, 780.0), Some(bar_button()), WINDOW);
        assert_eq!(fit.side, Side::Below);
        // The window, less the button's top and the offset, less the margin kept at the bottom.
        assert_eq!(fit.max_height, WINDOW - 8.0 - 32.0 - MARGIN);
        assert!(8.0 + fit.offset + fit.max_height <= WINDOW - MARGIN);
    }

    #[test]
    fn a_menu_that_does_not_fit_below_opens_above_when_there_is_more_room_there() {
        let low = Extent {
            top: 560.0,
            bottom: 588.0,
        };
        let fit = fit(below(32.0, 420.0), Some(low), WINDOW);
        assert_eq!(fit.side, Side::Above);
        assert_eq!(fit.offset, -4.0);
        assert_eq!(fit.max_height, 420.0);
    }

    #[test]
    fn a_menu_stays_on_the_side_with_more_room_when_neither_has_enough() {
        let middle = Extent {
            top: 300.0,
            bottom: 328.0,
        };
        // Below: 628 - 332 - 8 = 288; above: 296 - 8 = 288; a tie keeps the side asked for.
        let tie = fit(below(32.0, 420.0), Some(middle), WINDOW);
        assert_eq!(tie.side, Side::Below);
        let lower = Extent {
            top: 400.0,
            bottom: 428.0,
        };
        let more_above = fit(below(32.0, 600.0), Some(lower), WINDOW);
        assert_eq!(more_above.side, Side::Above);
        assert_eq!(more_above.max_height, 400.0 - 4.0 - MARGIN);
    }

    #[test]
    fn a_composer_menu_opens_above_its_chip_and_drops_below_when_the_chip_is_at_the_top() {
        let chip = Extent {
            top: 520.0,
            bottom: 548.0,
        };
        let up = fit(above(420.0), Some(chip), WINDOW);
        assert_eq!(up.side, Side::Above);
        assert_eq!(up.offset, -4.0);
        assert_eq!(up.max_height, 420.0);
        let top = Extent {
            top: 40.0,
            bottom: 68.0,
        };
        let down = fit(above(420.0), Some(top), WINDOW);
        assert_eq!(down.side, Side::Below);
        // Under the chip: its height and the gap, from its top edge.
        assert_eq!(down.offset, 28.0 + 4.0);
        assert_eq!(down.max_height, 420.0);
    }

    #[test]
    fn a_menu_is_never_taller_than_the_window_less_its_margins() {
        let fit = fit(
            above(780.0),
            Some(Extent {
                top: 600.0,
                bottom: 628.0,
            }),
            640.0,
        );
        assert!(fit.max_height <= 640.0 - 2.0 * MARGIN);
        let unmeasured = super::fit(below(32.0, 780.0), None, 640.0);
        assert_eq!(unmeasured.max_height, 640.0 - 2.0 * MARGIN);
        assert_eq!(unmeasured.side, Side::Below);
    }

    #[test]
    fn a_window_that_has_not_been_measured_gives_the_menu_all_it_asks_for() {
        let unmeasured = fit(below(32.0, 420.0), None, f32::MAX);
        assert_eq!(unmeasured.max_height, 420.0);
    }

    #[test]
    fn a_panel_at_a_fixed_place_is_as_tall_as_the_window_below_it() {
        assert_eq!(height_from(36.0, WINDOW), WINDOW - 36.0 - MARGIN);
        assert_eq!(height_from(1000.0, WINDOW), 0.0);
    }
}
