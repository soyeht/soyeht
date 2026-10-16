//! The find bar of the editor: the words, the three toggles, how many matches there are and
//! where the current one is, the buttons that step through them, and the one that closes the bar.
//! The engine searches the whole file; the bar only asks.

use super::{
    prelude::*,
    ui::{acting, control_band, hint, owned},
};
use crate::{
    editor_ui::FindBar,
    theme::{Palette, desktop, editor, pane_interior, radius, space, type_scale},
};
use soyeht_protocol::{FileFindResult, Pane};

/// "3 of 41", "No results", or nothing before there is a search.
fn status(find: &FindBar, result: Option<&FileFindResult>) -> String {
    if find.query.is_empty() {
        return String::new();
    }
    match result {
        Some(result) => match (&result.current, result.total) {
            (Some(current), total) if total > 0 => {
                format!("{} of {total}", u64::from(current.index) + 1)
            }
            _ => "No results".to_owned(),
        },
        None => String::new(),
    }
}

/// One of the toggles of the bar.
#[derive(Clone, Copy)]
enum Toggle {
    Case,
    Word,
    Regex,
}

impl Toggle {
    fn label(self) -> &'static str {
        match self {
            Self::Case => "Aa",
            Self::Word => "ab",
            Self::Regex => ".*",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Case => "Match case",
            Self::Word => "Match whole word",
            Self::Regex => "Use regular expression",
        }
    }

    fn of(self, find: &mut FindBar) -> &mut bool {
        match self {
            Self::Case => &mut find.case_sensitive,
            Self::Word => &mut find.whole_word,
            Self::Regex => &mut find.regex,
        }
    }
}

/// A button of the bar that steps through the matches or closes the bar.
struct Step {
    glyph: Icon,
    tip: &'static str,
    enabled: bool,
    act: fn(&mut Workbench, &str, &mut Context<Workbench>),
}

impl Workbench {
    /// The bar, when it is open.
    pub(super) fn find_bar(
        &mut self,
        pane: &Pane,
        cx: &mut Context<Self>,
        palette: &'static Palette,
    ) -> Option<AnyElement> {
        let find = self.panes.get(&pane.pane_id)?.editor.find.clone();
        if !find.open {
            return None;
        }
        let result = self
            .state
            .editors
            .file_of_pane(&pane.pane_id)
            .and_then(|file| file.search.result.as_ref());
        let words = status(&find, result);
        let matches = result.is_some_and(|result| result.total > 0);
        let toggles = [Toggle::Case, Toggle::Word, Toggle::Regex]
            .map(|toggle| self.find_toggle(&pane.pane_id, toggle, &find, palette, cx));
        let steps = [
            Step {
                glyph: Icon::ChevronUp,
                tip: "Previous match",
                enabled: matches,
                act: Workbench::find_previous,
            },
            Step {
                glyph: Icon::ChevronDown,
                tip: "Next match",
                enabled: matches,
                act: Workbench::find_next,
            },
            Step {
                glyph: Icon::Close,
                tip: "Close (Esc)",
                enabled: true,
                act: Workbench::find_close,
            },
        ]
        .map(|step| find_step(&pane.pane_id, step, palette, cx));
        Some(
            control_band(palette)
                .child(icon(Icon::Search, desktop::INLINE_GLYPH, palette.secondary))
                .child(find_field(&pane.pane_id, &find, palette, cx))
                .children(toggles)
                .child(
                    div()
                        .flex_none()
                        .min_w(px(desktop::EDITOR_FIND_STATUS_WIDTH))
                        .flex()
                        .justify_end()
                        .mono()
                        .text_size(px(type_scale::SMALL))
                        .text_color(rgb(palette.secondary))
                        .child(words),
                )
                .children(steps)
                .into_any_element(),
        )
    }

    fn find_toggle(
        &mut self,
        pane_id: &str,
        toggle: Toggle,
        find: &FindBar,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut probe = find.clone();
        let on = *toggle.of(&mut probe);
        let color = if on {
            palette.accent
        } else {
            palette.secondary
        };
        row()
            .id(id(&format!("editor-find-{}", toggle.label()), pane_id))
            .flex_none()
            .h(px(editor::FIND_FIELD_HEIGHT))
            .px(px(space::X6))
            .rounded_full()
            .cursor_pointer()
            .own_press()
            .when(on, |chip| chip.bg(rgb(palette.well)))
            .text_color(rgb(color))
            .text_size(px(type_scale::SMALL))
            .font_weight(FontWeight::BOLD)
            .mono()
            .tooltip(hint(toggle.tip(), palette))
            .on_click(owned(
                cx,
                pane_id,
                move |this, pane, _: &ClickEvent, _, cx| {
                    let flag = toggle.of(&mut this.panes.of(pane).editor.find);
                    *flag = !*flag;
                    this.search_changed(pane);
                    cx.notify();
                },
            ))
            .child(toggle.label())
            .into_any_element()
    }
}

/// The field with the words, which takes the keys when it is clicked.
fn find_field(
    pane_id: &str,
    find: &FindBar,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let words = if find.query.is_empty() {
        div().text_color(rgb(palette.muted)).child("Find")
    } else {
        div()
            .truncate()
            .text_color(rgb(palette.ink))
            .child(SharedString::from(find.query.clone()))
    };
    let caret = div()
        .flex_none()
        .w(px(desktop::EDITOR_CARET_WIDTH))
        .h(px(type_scale::SMALL))
        .bg(rgb(palette.accent));
    row()
        .id(id("editor-find-field", pane_id))
        .flex_1()
        .min_w_0()
        .h(px(editor::FIND_FIELD_HEIGHT))
        .px(px(space::X8))
        .rounded(px(radius::COMPACT))
        .bg(rgb(palette.face))
        .mono()
        .text_size(px(type_scale::SMALL))
        .cursor_text()
        .own_press()
        .on_click(acting(cx, pane_id, Workbench::find_focus))
        .child(words)
        .when(find.focused, |field| field.child(caret))
        .into_any_element()
}

fn find_step(
    pane_id: &str,
    step: Step,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let button = row()
        .id(id(&format!("editor-find-{}", step.tip), pane_id))
        .flex_none()
        .size(px(editor::FIND_FIELD_HEIGHT))
        .justify_center()
        .own_press()
        .tooltip(hint(step.tip, palette))
        .child(icon(step.glyph, desktop::INLINE_GLYPH, palette.secondary));
    if step.enabled {
        button
            .cursor_pointer()
            .on_click(acting(cx, pane_id, step.act))
            .into_any_element()
    } else {
        button
            .opacity(pane_interior::DISABLED_OPACITY)
            .into_any_element()
    }
}
