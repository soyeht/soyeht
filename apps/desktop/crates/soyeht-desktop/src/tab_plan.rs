//! Which workspace tabs fit in the top bar. The Mac measures its tabs; here a tab's width is
//! estimated from its name, which is close enough to decide what goes behind the "+N" button.

use crate::theme::{desktop, glyph, layout, space};

/// Width of one character of a tab name, in points, at the tab label size.
const CHARACTER_WIDTH: f32 = desktop::TAB_CHARACTER_WIDTH;
/// What a tab adds around its name: padding on both sides, the close mark and the gaps.
const FRAME: f32 = 2.0 * space::X12 + space::X6 + glyph::TAB_CLOSE + 2.0 * space::X2;
const MINIMUM_WIDTH: f32 = desktop::TAB_MINIMUM_WIDTH;

/// The estimated width of a tab named `name`.
pub fn tab_width(name: &str) -> f32 {
    (name.chars().count() as f32 * CHARACTER_WIDTH + FRAME)
        .clamp(MINIMUM_WIDTH, layout::WORKSPACE_TAB_MAXIMUM_WIDTH)
}

/// The tabs shown and the tabs behind "+N", as indices into the workspace list. The active tab
/// is always shown; the others fill the room in order.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub visible: Vec<usize>,
    pub hidden: Vec<usize>,
}

pub fn plan(names: &[&str], active: Option<usize>, available: f32) -> Plan {
    let gap = space::X10;
    let overflow = layout::WORKSPACE_OVERFLOW_BUTTON_WIDTH + gap;
    let widths: Vec<f32> = names.iter().map(|name| tab_width(name) + gap).collect();
    let fits_all = widths.iter().sum::<f32>() <= available;
    let room = if fits_all {
        available
    } else {
        (available - overflow).max(0.0)
    };
    let mut used = active.map_or(0.0, |index| widths[index]);
    let mut visible: Vec<usize> = active.into_iter().collect();
    for (index, width) in widths.iter().enumerate() {
        if Some(index) == active {
            continue;
        }
        if used + width <= room {
            used += width;
            visible.push(index);
        }
    }
    visible.sort_unstable();
    let hidden = (0..names.len())
        .filter(|index| !visible.contains(index))
        .collect();
    Plan { visible, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_are_narrower_than_long_ones_and_capped() {
        assert!(tab_width("api") < tab_width("a long workspace name"));
        assert_eq!(
            tab_width(&"x".repeat(80)),
            layout::WORKSPACE_TAB_MAXIMUM_WIDTH
        );
        assert_eq!(tab_width(""), MINIMUM_WIDTH);
    }

    #[test]
    fn everything_fits_in_a_wide_bar() {
        let plan = plan(&["a", "b", "c"], Some(1), 1000.0);
        assert_eq!(plan.visible, [0, 1, 2]);
        assert!(plan.hidden.is_empty());
    }

    #[test]
    fn what_does_not_fit_goes_behind_the_overflow_button() {
        let names = ["one", "two", "three", "four", "five", "six"];
        let plan = plan(&names, Some(0), 3.0 * tab_width("one") + 100.0);
        assert!(plan.visible.contains(&0));
        assert!(!plan.hidden.is_empty());
        assert_eq!(plan.visible.len() + plan.hidden.len(), names.len());
    }

    #[test]
    fn the_active_tab_stays_visible_even_when_it_is_the_last_one() {
        let names = ["one", "two", "three", "four", "five", "six"];
        let plan = plan(&names, Some(5), 2.0 * tab_width("one") + 100.0);
        assert!(plan.visible.contains(&5));
    }

    #[test]
    fn no_room_hides_every_other_tab() {
        let plan = plan(&["a", "b"], Some(0), 0.0);
        assert_eq!(plan.visible, [0]);
        assert_eq!(plan.hidden, [1]);
    }
}
