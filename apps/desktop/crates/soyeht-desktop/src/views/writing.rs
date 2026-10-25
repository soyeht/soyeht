//! A text, presentation or screenplay pane, as the Mac draws it: a header with the file's name and
//! its words (its scenes for a screenplay, its slides for a presentation), the text in a column of
//! reading width, or the page of a screenplay, or the slides of a presentation, and over a
//! selected part of a text the toolbar of its formats. The card is the only raised surface; the
//! text is flat. The text is a field (`composer_field`), and what is typed reaches the engine as
//! `writing_flow` says.

use super::{
    editor::conflict_banner,
    menus,
    pane::{Action, notice_body},
    prelude::*,
    ui::{header_bar, hint, pressable},
};
use crate::{
    neo,
    open_menu::Menu,
    theme::{Palette, desktop, elevation, fonts, pane_layout, radius, space, type_scale},
};
use gpui::{Anchor, relative};
use soyeht_client::{
    Connection,
    editor::{EditorFile, PaneLoad},
    writing::rules::{self, Counted, SelectionAction},
};
use soyeht_protocol::{Pane, WritingKind};

/// The icon of a button of the selection toolbar.
fn glyph(action: SelectionAction) -> Icon {
    match action {
        SelectionAction::Bold => Icon::Bold,
        SelectionAction::Italic => Icon::Italic,
        SelectionAction::Heading => Icon::Heading,
        SelectionAction::List => Icon::List,
        SelectionAction::Quote => Icon::Quote,
        SelectionAction::Link => Icon::Hash,
        SelectionAction::Image => Icon::Image,
    }
}

/// The bar a document's place or pane starts with: the mark of its section and the title.
pub(super) fn titled_bar(palette: &Palette, mark: Icon, title: impl IntoElement) -> Div {
    header_bar(pane_layout::HEADER_HEIGHT)
        .child(icon(mark, desktop::INLINE_GLYPH, palette.ink))
        .child(title)
}

/// The face of a title in that bar.
pub(super) fn title_face(palette: &Palette) -> Div {
    div()
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_size(px(type_scale::BODY))
        .text_color(rgb(palette.ink))
}

impl Workbench {
    pub(super) fn writing_header(
        &mut self,
        palette: &'static Palette,
        pane: &Pane,
        cx: &mut Context<Self>,
    ) -> Div {
        let file = self.state.editors.file_of_pane(&pane.pane_id);
        let name = rules::file_name(file.map(|file| file.doc.path.as_str()), &pane.name);
        let mark = match pane.writing_kind {
            Some(WritingKind::Screenplay) => Icon::Clapperboard,
            Some(WritingKind::Presentation) => Icon::Copy,
            _ => Icon::Feather,
        };
        let document = self.state.writings.of_pane(&pane.pane_id);
        let count = match pane.writing_kind {
            Some(WritingKind::Screenplay) => document
                .and_then(|document| document.scenes())
                .map(|scenes| rules::count_label(u64::from(scenes), Counted::Scene)),
            Some(WritingKind::Presentation) => document
                .and_then(|document| document.slides())
                .map(|slides| rules::count_label(slides.len() as u64, Counted::Slide)),
            _ => document
                .and_then(|document| document.words())
                .map(|words| rules::count_label(u64::from(words), Counted::Word)),
        };
        let present =
            (pane.writing_kind == Some(WritingKind::Presentation)).then_some(Action::Present);
        let buttons: Vec<AnyElement> = present
            .into_iter()
            .chain([Action::Companion, Action::Close])
            .map(|action| self.header_button(pane, action, palette, cx))
            .collect();
        let title = title_face(palette)
            .min_w_0()
            .max_w(px(desktop::PANE_TITLE_MAX_WIDTH))
            .id(id("writing-title", &pane.pane_id))
            .truncate()
            .child(SharedString::from(name));
        titled_bar(palette, mark, title)
            .children(count.map(|count| {
                div()
                    .flex_none()
                    .text_size(px(type_scale::SMALL))
                    .text_color(rgb(palette.secondary))
                    .child(format!("\u{b7} {count}"))
            }))
            .child(spacer())
            .child(self.file_state_word(&pane.pane_id, palette))
            .children(buttons)
    }

    pub(super) fn writing_body(&mut self, cx: &mut Context<Self>, pane: &Pane) -> AnyElement {
        let palette = self.theme.palette();
        let pane_id = pane.pane_id.clone();
        let Some(file) = self.state.editors.file_of_pane(&pane_id) else {
            return notice_body(palette, LOADING);
        };
        if file.page.is_none() {
            return match self.state.editors.pane_load(&pane_id) {
                PaneLoad::Failed(reason) => Self::load_failed(reason, &pane_id, palette, cx),
                PaneLoad::Loading | PaneLoad::Ready => notice_body(palette, LOADING),
            };
        }
        let (lines, conflicted) = (file.lines.max(1), file.doc.conflicted);
        // A document is edited whole: the lines the page does not hold yet are read, and the
        // words of a text that settled are counted.
        let reading = self.state.editors.show(&pane_id, 0, lines);
        let counting = self.state.writing_tick();
        self.run_effects(reading);
        self.run_effects(counting);
        let unfit = self
            .state
            .editors
            .file_of_pane(&pane_id)
            .and_then(EditorFile::why_not_editable);
        let content = match unfit {
            None if self.writing_kind(&pane_id) == Some(WritingKind::Presentation) => {
                self.presentation_content((palette, &pane_id), cx)
            }
            None => self.writing_text(cx, &pane_id, palette),
            Some(reason) => notice_body(palette, reason),
        };
        column()
            .flex_1()
            .min_h_0()
            .children(conflicted.then(|| conflict_banner(palette)))
            .child(content)
            .into_any_element()
    }

    /// The text in its column (the page, for a screenplay), scrolled to the caret, with the
    /// toolbar over a selection of a text.
    fn writing_text(
        &mut self,
        cx: &mut Context<Self>,
        pane_id: &str,
        palette: &'static Palette,
    ) -> AnyElement {
        self.sync_writing_field(pane_id, cx);
        let field = self.writing_field(cx, pane_id);
        field.update(cx, |field, _| field.style(palette, "Start writing"));
        let screenplay = self.writing_kind(pane_id) == Some(WritingKind::Screenplay);
        if screenplay {
            let styles = self
                .state
                .writings
                .line_styles(pane_id, field.read(cx).draft().text());
            field.update(cx, |field, _| field.set_page(styles));
        }
        // The toolbar belongs to the pane that has the keyboard: a selection left in another
        // pane waits there without it.
        let focused = self
            .state
            .focused_pane()
            .is_some_and(|pane| pane.pane_id == pane_id);
        let selected = focused && !screenplay && field.read(cx).draft().has_selection();
        let scroll = self.panes.of(pane_id).writing.scroll.clone();
        let toolbar = selected.then(|| self.writing_toolbar(palette, pane_id, cx));
        let column = if screenplay {
            // The page of a screenplay is set in Courier Prime at the Mac's size, and its margins
            // are the Mac's share of the page, so that a narrow pane keeps the same proportions.
            div()
                .w_full()
                .max_w(px(desktop::SCREENPLAY_PAGE_WIDTH))
                .py(px(space::X18))
                .child(
                    div()
                        .mx(relative(desktop::SCREENPLAY_PAGE_INSET_SHARE))
                        .font_family(fonts::SCREENPLAY)
                        .text_size(px(desktop::SCREENPLAY_TEXT_SIZE))
                        .line_height(px(desktop::SCREENPLAY_LINE_HEIGHT))
                        .child(field),
                )
        } else {
            div()
                .w_full()
                .max_w(px(desktop::WRITING_COLUMN_WIDTH))
                .p(px(space::X18))
                .text_size(px(desktop::WRITING_TEXT_SIZE))
                .line_height(relative(desktop::WRITING_LINE_HEIGHT))
                .child(field)
        }
        .text_color(rgb(palette.ink));
        let page = div()
            .id(id("writing-text", pane_id))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(
                div()
                    .w_full()
                    .min_h_full()
                    .flex()
                    .justify_center()
                    .cursor_text()
                    .child(column),
            );
        // The toolbar floats over the top of the text, in the middle.
        let floating = toolbar.map(|toolbar| {
            div()
                .absolute()
                .top(px(space::X8))
                .w_full()
                .flex()
                .justify_center()
                .child(toolbar)
        });
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(page)
            .children(floating)
            .into_any_element()
    }

    /// The formats of the selected text, as one capsule over the text.
    fn writing_toolbar(
        &mut self,
        palette: &'static Palette,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let connected = self.state.connection == Connection::Connected;
        // The desktop client does not ask for an address or a path yet (a link, an image).
        let buttons: Vec<AnyElement> = rules::selection_actions(true)
            .iter()
            .filter(|action| !action.asks_destination())
            .map(|action| self.toolbar_button(cx, palette, (pane_id, *action), connected))
            .collect();
        neo::raised(palette, radius::PILL, elevation::CHIP)
            .occlude()
            .child(
                row()
                    .h(px(desktop::WRITING_TOOLBAR_HEIGHT))
                    .px(px(space::X6))
                    .gap(px(space::X4))
                    .children(buttons),
            )
            .into_any_element()
    }

    fn toolbar_button(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        (pane_id, action): (&str, SelectionAction),
        enabled: bool,
    ) -> AnyElement {
        let formats = action.formats();
        let menu = Menu::Format {
            pane_id: pane_id.to_owned(),
            action: action.name(),
        };
        let open = self.menus.is_open(&menu);
        let owner = pane_id.to_owned();
        let single = (formats.len() == 1).then(|| formats[0].1.clone());
        let press = menu.clone();
        let control = pressable(
            row()
                .id(id(&format!("writing-{}", action.name()), pane_id))
                .w(px(desktop::WRITING_TOOLBAR_BUTTON_WIDTH))
                .h(px(desktop::WRITING_TOOLBAR_HEIGHT))
                .justify_center()
                .rounded(px(radius::COMPACT))
                .bg(rgb(if open { palette.hover } else { palette.face }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(palette.hover)))
                .tooltip(hint(action.title(), palette)),
            enabled,
            cx,
            move |this, cx| match &single {
                Some(kind) => this.format_selection(&owner, kind.clone(), cx),
                None => this.toggle_menu(press.clone(), cx),
            },
        )
        .child(icon(
            glyph(action),
            desktop::HEADER_ACTION_GLYPH,
            palette.ink,
        ));
        let popup = (open && formats.len() > 1).then(|| {
            let rows = formats
                .iter()
                .map(|(label, kind)| {
                    let (owner, kind) = (pane_id.to_owned(), kind.clone());
                    menus::on_pick(
                        menus::item(
                            palette,
                            &format!("writing-{}-{label}", action.name()),
                            *label,
                            false,
                        ),
                        cx,
                        move |this, cx| this.format_selection(&owner, kind.clone(), cx),
                    )
                })
                .collect();
            menus::popup(
                palette,
                Anchor::TopLeft,
                (0.0, desktop::WRITING_TOOLBAR_HEIGHT + space::X4),
                menus::list(rows),
                cx,
            )
        });
        div()
            .relative()
            .flex_none()
            .child(control)
            .children(popup)
            .into_any_element()
    }
}

/// What stands where the text would be while its document is on its way.
const LOADING: &str = "Loading the document";
