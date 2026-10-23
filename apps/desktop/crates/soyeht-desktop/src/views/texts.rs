//! The Texts place, as the Mac's section view draws it: the section's name and what it holds, a
//! button that makes a document, a line that narrows the list, and the list. A document opens in
//! a pane, which the agents' place shows. The rules (names, search, the form) are
//! `soyeht_client::writing::rules`, whose cases the Mac passes too.

use super::{
    cards::{Role, button},
    prelude::*,
    switch_popover::well_row,
    ui::{caption, chrome_text, empty_state, mono_note, pane_card_shell, pressable},
    writing::{title_face, titled_bar},
};
use crate::{
    composer_field::ComposerField,
    theme::{Palette, desktop, radius, space, type_scale},
    writing_flow::TEXTS,
};
use gpui::Entity;
use soyeht_client::{
    Connection,
    writing::{SectionList, rules},
};
use soyeht_protocol::WritingSectionEntry;

impl Workbench {
    /// The Texts place of the selected workspace.
    pub(super) fn texts_surface(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        workspace: &str,
    ) -> AnyElement {
        self.texts_stage(workspace, cx);
        let search = self.texts_search(cx);
        search.update(cx, |field, _| field.style(palette, "Search this section"));
        let query = search.read(cx).draft().text().to_owned();
        let list = self
            .state
            .writings
            .section(workspace, &TEXTS)
            .cloned()
            .unwrap_or_default();
        let connected = self.state.connection == Connection::Connected;
        let page = column()
            .flex_1()
            .min_h_0()
            .w_full()
            .max_w(px(desktop::WRITING_LIST_WIDTH))
            .gap(px(space::X14))
            .px(px(space::X18))
            .py(px(space::X14))
            .child(self.texts_title(cx, connected, palette))
            .children(self.texts_form(palette, cx))
            .child(search_line(search, palette))
            .child(self.texts_list(workspace, &list, &query, palette, cx));
        pane_card_shell(palette)
            .child(titled_bar(
                palette,
                title_face(palette).child(rules::title(&TEXTS)),
            ))
            .child(column().flex_1().min_h_0().items_center().child(page))
            .into_any_element()
    }

    /// The section's name and what it holds, and the button that makes a document.
    fn texts_title(
        &mut self,
        cx: &mut Context<Self>,
        connected: bool,
        palette: &'static Palette,
    ) -> Div {
        let make = pressable(
            row()
                .id("texts-new")
                .cursor_pointer()
                .px(px(space::X12))
                .py(px(space::X6))
                .gap(px(space::X6))
                .rounded_full()
                .bg(rgb(palette.ink))
                .text_color(rgb(palette.face))
                .text_size(px(type_scale::CONTROL))
                .font_weight(FontWeight::BOLD),
            connected,
            cx,
            |this, cx| this.texts_new(cx),
        )
        .child(icon(Icon::Plus, desktop::INLINE_GLYPH, palette.face))
        .child("New document");
        row()
            .items_end()
            .child(
                column()
                    .gap(px(space::X4))
                    .flex_1()
                    .min_w_0()
                    .child(
                        chrome_text(FontWeight::BOLD, type_scale::TITLE)
                            .text_color(rgb(palette.ink))
                            .child(rules::title(&TEXTS)),
                    )
                    .child(caption(palette).child(rules::subtitle(&TEXTS))),
            )
            .child(make)
    }

    /// The form that makes a document, while it is open.
    fn texts_form(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Option<Div> {
        let form = self.texts.form.as_ref()?;
        let (title, folder) = (form.title.clone(), form.folder.clone());
        title.update(cx, |field, _| field.style(palette, "Title"));
        folder.update(cx, |field, _| field.style(palette, "Subfolder (optional)"));
        let creating = self
            .state
            .writings
            .section(&self.state.selected_workspace_id()?, &TEXTS)
            .is_some_and(|list| list.creating);
        let ready = !title.read(cx).draft().is_blank()
            && !creating
            && self.state.connection == Connection::Connected;
        let cancel = pressable(
            button(
                id("texts-form", "cancel"),
                Role::Secondary,
                "Cancel",
                palette,
            ),
            true,
            cx,
            |this, cx| this.texts_close_form(cx),
        );
        let create = pressable(
            button(id("texts-form", "create"), Role::Primary, "Create", palette),
            ready,
            cx,
            |this, cx| this.texts_create(cx),
        );
        Some(
            column()
                .flex_none()
                .gap(px(space::X10))
                .p(px(space::X14))
                .rounded(px(radius::CONTROL))
                .bg(rgb(palette.well))
                .child(
                    chrome_text(FontWeight::BOLD, type_scale::SUBHEADING)
                        .text_color(rgb(palette.ink))
                        .child(rules::new_document_title(&TEXTS)),
                )
                .child(field_line(title, palette))
                .child(field_line(folder, palette))
                .child(caption(palette).child(rules::new_document_note(&TEXTS)))
                .child(
                    row()
                        .gap(px(space::X8))
                        .child(cancel)
                        .child(div().flex_1())
                        .child(create),
                ),
        )
    }

    /// The documents of the section that the search keeps, or what stands in their place.
    fn texts_list(
        &mut self,
        workspace: &str,
        list: &SectionList,
        query: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if list.failed {
            return self.texts_failed(palette, workspace, cx);
        }
        if list.loading && list.entries.is_empty() {
            return empty_state(palette, None, "Loading", None).into_any_element();
        }
        let kept = rules::search(&list.entries, query);
        if kept.is_empty() {
            let (heading, detail) = rules::empty_section(&TEXTS, !query.trim().is_empty());
            return empty_state(
                palette,
                Some(Icon::Feather),
                heading,
                detail.map(Into::into),
            )
            .into_any_element();
        }
        let mut rows: Vec<AnyElement> = kept
            .into_iter()
            .map(|entry| self.texts_row(workspace, entry, palette, cx))
            .collect();
        if list.next.is_some() {
            let workspace = workspace.to_owned();
            rows.push(
                pressable(
                    button(
                        id("texts-more", &workspace),
                        Role::Secondary,
                        "Load more",
                        palette,
                    ),
                    true,
                    cx,
                    move |this, cx| this.texts_more(&workspace, cx),
                )
                .into_any_element(),
            );
        }
        div()
            .id("texts-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(column().gap(px(space::X4)).children(rows))
            .into_any_element()
    }

    /// A document of the list: its mark, its title and where it is kept. A click opens it.
    fn texts_row(
        &mut self,
        workspace: &str,
        entry: &WritingSectionEntry,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (workspace, path) = (workspace.to_owned(), entry.path.clone());
        let mark = row()
            .size(px(desktop::WRITING_ROW_ICON_BOX))
            .flex_none()
            .justify_center()
            .rounded(px(radius::COMPACT))
            .bg(rgb(palette.well))
            .child(icon(
                Icon::Feather,
                desktop::HEADER_ACTION_GLYPH,
                palette.accent,
            ));
        let words = column()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(type_scale::BODY))
                    .text_color(rgb(palette.ink))
                    .child(SharedString::from(entry.title.clone())),
            )
            .child(mono_note(palette).child(SharedString::from(entry.path.clone())));
        pressable(
            row()
                .id(id("texts-row", &entry.path))
                .gap(px(space::X12))
                .p(px(space::X10))
                .rounded(px(radius::CONTROL))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(palette.hover))),
            true,
            cx,
            move |this, cx| this.texts_open(&workspace, &path, cx),
        )
        .child(mark)
        .child(words)
        .child(icon(
            Icon::ChevronRight,
            desktop::CHEVRON_GLYPH,
            palette.secondary,
        ))
        .into_any_element()
    }

    /// The list could not be read: what stands in its place, and a button that asks again.
    fn texts_failed(
        &mut self,
        palette: &'static Palette,
        workspace: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = workspace.to_owned();
        empty_state(
            palette,
            Some(Icon::Alert),
            "The texts could not be listed",
            Some("The engine did not answer. Try again.".into()),
        )
        .child(pressable(
            button(
                id("texts-again", &workspace),
                Role::Secondary,
                "Try again",
                palette,
            ),
            true,
            cx,
            move |this, cx| this.texts_retry(&workspace, cx),
        ))
        .into_any_element()
    }
}

/// The well that holds a line of the form.
fn field_line(field: Entity<ComposerField>, palette: &Palette) -> Div {
    well_row(palette)
        .bg(rgb(palette.face))
        .h(px(desktop::SEARCH_FIELD_HEIGHT))
        .child(div().flex_1().min_w_0().overflow_hidden().child(field))
}

/// The search line: a glyph and the field, in a well.
fn search_line(field: Entity<ComposerField>, palette: &Palette) -> Div {
    well_row(palette)
        .flex_none()
        .gap(px(space::X8))
        .h(px(desktop::SEARCH_FIELD_HEIGHT))
        .child(icon(Icon::Search, desktop::INLINE_GLYPH, palette.secondary))
        .child(div().flex_1().min_w_0().overflow_hidden().child(field))
}
