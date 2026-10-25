//! The place of a section of documents (Texts, Screenplays), as the Mac's section view draws it:
//! the section's name and what it holds, a button that makes a document, a line that narrows the
//! list, and the list. A document opens in a pane, which the agents' place shows. The rules
//! (names, search, the form) are `soyeht_client::writing::rules`, whose cases the Mac passes too.

use super::{
    cards::{Role, button},
    prelude::*,
    switch_popover::well_row,
    ui::{caption, chrome_text, empty_state, mono_note, pane_card_shell, pressable},
    writing::{title_face, titled_bar},
};
use crate::{
    composer_field::{ComposerField, focus_on_press},
    destination::Destination,
    theme::{Palette, desktop, layout, radius, space, type_scale},
};
use gpui::Entity;
use soyeht_client::{
    Connection,
    writing::{SectionList, rules},
};
use soyeht_protocol::{WritingSection, WritingSectionEntry};

impl Workbench {
    /// The place of a section, for the selected workspace.
    pub(super) fn place_surface(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        workspace: &str,
        section: &WritingSection,
    ) -> AnyElement {
        self.place_stage(section, workspace, cx);
        let search = self.place_search(section, cx);
        search.update(cx, |field, _| field.style(palette, "Search this section"));
        let query = search.read(cx).draft().text().to_owned();
        let list = self
            .state
            .writings
            .section(workspace, section)
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
            .child(self.place_title(cx, connected, palette, section))
            .children(self.place_form(palette, section, cx))
            .child(search_line(search, palette))
            .child(self.place_list(workspace, section, &list, &query, palette, cx));
        pane_card_shell(palette)
            .child(titled_bar(
                palette,
                Destination::of_section(section).icon(),
                title_face(palette).child(rules::title(section)),
            ))
            .child(column().flex_1().min_h_0().items_center().child(page))
            .into_any_element()
    }

    /// The section's name and what it holds, and the button that makes a document.
    fn place_title(
        &mut self,
        cx: &mut Context<Self>,
        connected: bool,
        palette: &'static Palette,
        section: &WritingSection,
    ) -> Div {
        let owner = section.clone();
        let make = pressable(
            row()
                .id(id("place-new", rules::name(section)))
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
            move |this, cx| this.place_new(&owner, cx),
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
                            .child(rules::title(section)),
                    )
                    .child(caption(palette).child(rules::subtitle(section))),
            )
            .child(make)
    }

    /// The form that makes a document, while it is open.
    fn place_form(
        &mut self,
        palette: &'static Palette,
        section: &WritingSection,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let form = self.places.of(section).form.as_ref()?;
        let (title, folder) = (form.title.clone(), form.folder.clone());
        title.update(cx, |field, _| field.style(palette, "Title"));
        folder.update(cx, |field, _| field.style(palette, "Subfolder (optional)"));
        let creating = self
            .state
            .writings
            .section(&self.state.selected_workspace_id()?, section)
            .is_some_and(|list| list.creating);
        let ready = !title.read(cx).draft().is_blank()
            && !creating
            && self.state.connection == Connection::Connected;
        let (closing, creating_in) = (section.clone(), section.clone());
        let cancel = pressable(
            button(
                id("place-form", "cancel"),
                Role::Secondary,
                "Cancel",
                palette,
            ),
            true,
            cx,
            move |this, cx| this.place_close_form(&closing, cx),
        );
        let create = pressable(
            button(id("place-form", "create"), Role::Primary, "Create", palette),
            ready,
            cx,
            move |this, cx| this.place_create(&creating_in, cx),
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
                        .child(rules::new_document_title(section)),
                )
                .child(field_line(title, palette))
                .child(field_line(folder, palette))
                .child(caption(palette).child(rules::new_document_note(section)))
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
    fn place_list(
        &mut self,
        workspace: &str,
        section: &WritingSection,
        list: &SectionList,
        query: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if list.failed {
            return self.place_failed(palette, workspace, section, cx);
        }
        if list.loading && list.entries.is_empty() {
            return empty_state(palette, None, "Loading", None).into_any_element();
        }
        let kept = rules::search(&list.entries, query);
        if kept.is_empty() {
            let (heading, detail) = rules::empty_section(section, !query.trim().is_empty());
            return empty_state(
                palette,
                Some(Destination::of_section(section).icon()),
                heading,
                detail.map(Into::into),
            )
            .into_any_element();
        }
        let mut rows: Vec<AnyElement> = kept
            .into_iter()
            .map(|entry| self.place_row(workspace, section, entry, palette, cx))
            .collect();
        if list.next.is_some() {
            let (workspace, section) = (workspace.to_owned(), section.clone());
            rows.push(
                pressable(
                    button(
                        id("place-more", &workspace),
                        Role::Secondary,
                        "Load more",
                        palette,
                    ),
                    true,
                    cx,
                    move |this, cx| this.place_page((&section, &workspace), true, cx),
                )
                .into_any_element(),
            );
        }
        div()
            .id("place-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(column().gap(px(space::X4)).children(rows))
            .into_any_element()
    }

    /// A document of the list: its mark, its title and where it is kept. A click opens it.
    fn place_row(
        &mut self,
        workspace: &str,
        section: &WritingSection,
        entry: &WritingSectionEntry,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (workspace, path, section) =
            (workspace.to_owned(), entry.path.clone(), section.clone());
        let mark_icon = Destination::of_section(&section).icon();
        let mark = row()
            .size(px(desktop::WRITING_ROW_ICON_BOX))
            .flex_none()
            .justify_center()
            .rounded(px(radius::COMPACT))
            .bg(rgb(palette.well))
            .child(icon(
                mark_icon,
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
                .id(id("place-row", &entry.path))
                .gap(px(space::X12))
                .p(px(space::X10))
                .rounded(px(radius::CONTROL))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(palette.hover))),
            true,
            cx,
            move |this, cx| this.place_open(&section, &workspace, &path, cx),
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
    fn place_failed(
        &mut self,
        palette: &'static Palette,
        workspace: &str,
        section: &WritingSection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (workspace, owner) = (workspace.to_owned(), section.clone());
        empty_state(
            palette,
            Some(Icon::Alert),
            format!(
                "The {} could not be listed",
                rules::title(section).to_lowercase()
            ),
            Some("The engine did not answer. Try again.".into()),
        )
        .child(pressable(
            button(
                id("place-again", &workspace),
                Role::Secondary,
                "Try again",
                palette,
            ),
            true,
            cx,
            move |this, cx| this.place_page((&owner, &workspace), false, cx),
        ))
        .into_any_element()
    }
}

/// The well that holds a line of the form.
fn field_line(field: Entity<ComposerField>, palette: &Palette) -> Div {
    focus_on_press(
        well_row(palette)
            .bg(rgb(palette.face))
            .h(px(layout::SEARCH_FIELD_HEIGHT)),
        &field,
    )
    .child(div().flex_1().min_w_0().overflow_hidden().child(field))
}

/// The search line: a glyph and the field, in a well.
fn search_line(field: Entity<ComposerField>, palette: &Palette) -> Div {
    focus_on_press(
        well_row(palette)
            .flex_none()
            .gap(px(space::X8))
            .h(px(layout::SEARCH_FIELD_HEIGHT)),
        &field,
    )
    .child(icon(Icon::Search, desktop::INLINE_GLYPH, palette.secondary))
    .child(div().flex_1().min_w_0().overflow_hidden().child(field))
}
