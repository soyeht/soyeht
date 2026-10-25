//! Writing: the text, presentation and screenplay panes and the places that list them. A pane holds its
//! document in a field; what is typed reaches the engine as the one edit that turns the page into
//! the field's text (`soyeht_client::writing`), and the engine saves the draft shortly after the
//! last edit. This carries what the person does to the replica and what the replica says back to
//! the field.

use crate::{
    composer_field::{ComposerEvent, ComposerField, ProseEvent, new_field, new_form_field},
    destination::Destination,
    workbench::Workbench,
    writing_style,
    writing_ui::NewDocument,
};
use gpui::{AppContext, Context, Entity, FocusHandle, Focusable};
use soyeht_client::Effect;
use soyeht_client::state::problem::kind;
use soyeht_client::writing::{
    document::line_start,
    presentation::{self, Show, ShowInput},
    rules::{self, DocumentDraft},
};
use soyeht_protocol::{
    PaneMode, WorkspacePaneLayoutAxis, WritingFormatKind, WritingKind, WritingSection,
};

impl Workbench {
    /// The kind of the document a writing pane shows.
    pub(crate) fn writing_kind(&self, pane_id: &str) -> Option<WritingKind> {
        self.state
            .pane(pane_id)
            .filter(|pane| pane.mode == PaneMode::Writing)
            .and_then(|pane| pane.writing_kind.clone())
    }

    /// Whether the pane is a text, a presentation or a screenplay: the documents the desktop
    /// client edits.
    pub(crate) fn is_document(&self, pane_id: &str) -> bool {
        self.writing_kind(pane_id).is_some()
    }

    /// The field that holds the text of a pane, made when first needed.
    pub(crate) fn writing_field(
        &mut self,
        cx: &mut Context<Self>,
        pane_id: &str,
    ) -> Entity<ComposerField> {
        let owner = pane_id.to_owned();
        if let Some(made) = self.panes.of(pane_id).writing.field.clone() {
            return made;
        }
        let ui = self.panes.of(pane_id);
        let (palette, scroll) = (self.theme.palette(), ui.writing.scroll.clone());
        let kind = self.writing_kind(pane_id);
        let field = match kind {
            Some(WritingKind::Screenplay) => {
                cx.new(|cx| ComposerField::screenplay(palette, scroll, cx))
            }
            Some(WritingKind::Presentation) => cx.new(|cx| {
                ComposerField::prose(palette, writing_style::presentation_highlights, scroll, cx)
            }),
            _ => cx.new(|cx| ComposerField::prose(palette, writing_style::highlights, scroll, cx)),
        };
        cx.subscribe(&field, move |this, _, event: &ProseEvent, cx| match event {
            ProseEvent::Edited => this.writing_edited(&owner, cx),
            // The source of a slide is Markdown but has no toolbar or format keys, as on the Mac.
            ProseEvent::Format(_) if kind == Some(WritingKind::Presentation) => {}
            ProseEvent::Format(kind) => this.format_selection(&owner, kind.clone(), cx),
            ProseEvent::Save => this.save_pane_file(&owner, cx),
            ProseEvent::Advance { tab } => this.screenplay_advance(&owner, *tab, cx),
        })
        .detach();
        self.panes.of(pane_id).writing.field = Some(field.clone());
        field
    }

    /// Where the keys of a writing pane go: its field, or the slide show while one is on.
    pub(crate) fn writing_focus(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        if !self.is_document(pane_id) {
            return None;
        }
        if self.panes.of(pane_id).writing.show.is_some() {
            return Some(self.show_focus(pane_id, cx));
        }
        Some(self.writing_field(cx, pane_id).focus_handle(cx))
    }

    /// The handle that takes the keys of a pane's slide show.
    pub(crate) fn show_focus(&mut self, pane_id: &str, cx: &mut Context<Self>) -> FocusHandle {
        self.panes
            .of(pane_id)
            .writing
            .show_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// What the field holds and has selected.
    fn field_text(
        &mut self,
        pane_id: &str,
        cx: &Context<Self>,
    ) -> Option<(String, (usize, usize))> {
        let field = self.panes.get(pane_id)?.writing.field.as_ref()?;
        let field = field.read(cx);
        Some((field.draft().text().to_owned(), field.selection_ends()))
    }

    /// The person typed or moved in the field.
    fn writing_edited(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some((text, selection)) = self.field_text(pane_id, cx) else {
            return;
        };
        match self.state.writing_retype(pane_id, &text, selection) {
            Ok(effects) => self.run_effects(effects),
            Err(reason) => {
                // What the engine would refuse is not kept: the field shows the document again.
                self.state.report(kind::WRITING_EDIT_REFUSED, reason);
                self.sync_writing_field(pane_id, cx);
            }
        }
        cx.notify();
    }

    /// The slides of a presentation pane, as the engine last split them.
    pub(crate) fn slides_of(&self, pane_id: &str) -> Vec<soyeht_protocol::WritingSlide> {
        self.state
            .writings
            .of_pane(pane_id)
            .and_then(|document| document.slides())
            .map(<[_]>::to_vec)
            .unwrap_or_default()
    }

    /// A slide of the rail was chosen: the caret goes to its first line.
    pub(crate) fn select_slide(&mut self, pane_id: &str, index: u32, cx: &mut Context<Self>) {
        let Some(slide) = self
            .slides_of(pane_id)
            .into_iter()
            .find(|slide| slide.index == index)
        else {
            return;
        };
        self.panes.of(pane_id).writing.slide = index;
        let field = self.writing_field(cx, pane_id);
        let at = line_start(field.read(cx).draft().text(), slide.start_line);
        field.update(cx, |field, cx| field.place_caret(at, cx));
        self.focus_request = Some(pane_id.to_owned());
        cx.notify();
    }

    /// Starts the slide show over the pane, at the slide the caret is on.
    pub(crate) fn present(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let count = self.slides_of(pane_id).len();
        if count == 0 {
            return;
        }
        let ui = &mut self.panes.of(pane_id).writing;
        ui.show = Some(Show::new(ui.slide as usize, count));
        self.focus_request = Some(pane_id.to_owned());
        cx.notify();
    }

    /// Closes the slide show and gives the keys back to the source.
    pub(crate) fn end_show(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.panes.of(pane_id).writing.show = None;
        self.focus_request = Some(pane_id.to_owned());
        cx.notify();
    }

    /// A key or a click in the slide show.
    pub(crate) fn show_input(
        &mut self,
        pane_id: &str,
        (input, command_held): (ShowInput, bool),
        cx: &mut Context<Self>,
    ) {
        let Some(command) = presentation::show_command(input, command_held) else {
            return;
        };
        let Some(show) = self.panes.of(pane_id).writing.show.as_mut() else {
            return;
        };
        if show.apply(command) {
            self.end_show(pane_id, cx);
        } else {
            cx.notify();
        }
    }

    /// Makes the field hold the text of the page, when it holds another: the engine changed it
    /// (an agent, a format, another client), or an edit was refused.
    pub(crate) fn sync_writing_field(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(text) = self
            .state
            .editors
            .file_of_pane(pane_id)
            .and_then(|file| file.whole_text())
        else {
            return;
        };
        let field = self.writing_field(cx, pane_id);
        if field.read(cx).draft().text() != text {
            field.update(cx, |field, cx| field.load(&text, cx));
            // The lines of a screenplay move with what the engine changed.
            self.state.writings.follow(pane_id, &text);
        }
    }

    /// Return or Tab in the page of a screenplay: the line stays and the next element starts
    /// below it, as the Mac does (`soyeht_client::writing::screenplay`).
    fn screenplay_advance(&mut self, pane_id: &str, tab: bool, cx: &mut Context<Self>) {
        let Some((text, (_, caret))) = self.field_text(pane_id, cx) else {
            return;
        };
        let Some((edit, next)) = self.state.screenplay_advance(pane_id, &text, caret, tab) else {
            return;
        };
        let field = self.writing_field(cx, pane_id);
        field.update(cx, |field, cx| {
            field.replace_range(edit.range.clone(), &edit.replacement, cx);
        });
        // The line the caret is on now starts as `next`, which the engine is asked again about
        // once the edit has reached it.
        self.writing_edited(pane_id, cx);
        if let Some((text, (_, caret))) = self.field_text(pane_id, cx) {
            let line = text
                .get(..caret)
                .map_or(0, |before| before.matches('\n').count());
            self.state.writings.assume(pane_id, line, next);
        }
        cx.notify();
    }

    /// Formats the selected text.
    pub(crate) fn format_selection(
        &mut self,
        pane_id: &str,
        kind: WritingFormatKind,
        cx: &mut Context<Self>,
    ) {
        self.menus.close();
        let selected = self.field_text(pane_id, cx);
        if let Some((text, selection)) = selected.filter(|_| self.require_connection(cx)) {
            let formatting = self.state.writing_format(pane_id, kind, &text, selection);
            self.run_effects(formatting);
            // The text comes back from the engine with its marks: the selection of the old text
            // would not be the same words.
            let field = self.writing_field(cx, pane_id);
            field.update(cx, |field, cx| field.collapse(cx));
        }
        cx.notify();
    }

    /// Asks the engine to save the file of a text, a screenplay or an editor tab, and waits for it:
    /// the draft is saved shortly after the last edit, and a file that cannot be saved says why.
    pub(crate) fn save_pane_file(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.editors.save(pane_id);
        self.run_effects(effects);
        cx.notify();
    }

    /// The close button of a text or a screenplay: the pane closes at once, or asks first when
    /// the document has edits not saved.
    pub(crate) fn request_close_writing(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(file) = self.state.editors.file_of_pane(pane_id) else {
            self.close_pane(pane_id);
            return;
        };
        let name = rules::file_name(Some(&file.doc.path), pane_id);
        let closing = rules::closing(&name, file.doc.dirty || file.has_unsent());
        self.ask_before_closing(pane_id, closing, cx);
    }

    /// Opens an agent beside a document, as a split of a document on the Mac does: the two are
    /// a pair, the agent working on the document the pane shows.
    pub(crate) fn open_companion(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if !self.require_connection(cx) {
            return;
        }
        // One agent is a document's companion, and one request is enough while it is on its way.
        if let Some(agent) = self.state.companion_of(pane_id) {
            self.run_effect(Effect::FocusPane(agent));
            return;
        }
        if self.state.companion_pending(pane_id) {
            return;
        }
        let Some(harness) = self.state.default_harness() else {
            self.state.report(
                kind::COMPANION_UNAVAILABLE,
                "No agent is ready to open beside this document. Check Preferences.",
            );
            cx.notify();
            return;
        };
        let ticket = self.next_ticket();
        if let Some(workspace_id) =
            self.state
                .expect_split(ticket, pane_id, WorkspacePaneLayoutAxis::Horizontal)
        {
            self.request_pane(workspace_id, Some(harness), ticket, cx);
        }
    }

    // The places that list the documents of a section.

    /// The place is on the stage: the first time in a row of frames its list is read again and
    /// what was left in its search and its form is cleared, as the Mac's section view starts over.
    pub(crate) fn place_stage(
        &mut self,
        section: &WritingSection,
        workspace_id: &str,
        cx: &mut Context<Self>,
    ) {
        self.places.leave(Some(section));
        let place = self.places.of_mut(section);
        if std::mem::replace(&mut place.on_stage, true) {
            return;
        }
        place.form = None;
        place.search = None;
        let new = std::mem::take(&mut place.new_on_entry);
        let effects = self.state.writing_list(workspace_id, section, false);
        self.run_effects(effects);
        if new {
            self.place_new(section, cx);
        }
        cx.notify();
    }

    /// The "Text", "Presentation" or "Screenplay" entry of the new-pane menu: the section's place, with the form
    /// that makes a document.
    pub(crate) fn open_new_document(&mut self, section: &WritingSection, cx: &mut Context<Self>) {
        self.menus.close();
        if self.require_connection(cx) {
            let place = self.places.of_mut(section);
            place.on_stage = false;
            place.new_on_entry = true;
            self.select_destination(Destination::of_section(section), cx);
        }
    }

    pub(crate) fn open_new_text(&mut self, cx: &mut Context<Self>) {
        self.open_new_document(&WritingSection::Texts, cx);
    }

    pub(crate) fn open_new_presentation(&mut self, cx: &mut Context<Self>) {
        self.open_new_document(&WritingSection::Presentations, cx);
    }

    pub(crate) fn open_new_screenplay(&mut self, cx: &mut Context<Self>) {
        self.open_new_document(&WritingSection::Screenplays, cx);
    }

    /// The line that narrows the list, made when first needed.
    pub(crate) fn place_search(
        &mut self,
        section: &WritingSection,
        cx: &mut Context<Self>,
    ) -> Entity<ComposerField> {
        if let Some(search) = &self.places.of(section).search {
            return search.clone();
        }
        let search = new_field(self.theme.palette(), cx);
        cx.subscribe(&search, |_, _, _: &ComposerEvent, cx| cx.notify())
            .detach();
        self.places.of_mut(section).search = Some(search.clone());
        search
    }

    /// Reads a page of the list again: the next one when `more`, else the first.
    pub(crate) fn place_page(
        &mut self,
        (section, workspace_id): (&WritingSection, &str),
        more: bool,
        cx: &mut Context<Self>,
    ) {
        let effects = self.state.writing_list(workspace_id, section, more);
        self.run_effects(effects);
        cx.notify();
    }

    /// Opens the document of the list that was chosen.
    pub(crate) fn place_open(
        &mut self,
        section: &WritingSection,
        workspace_id: &str,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        if self.require_connection(cx) {
            let opening = self.state.writing_open(workspace_id, section, path);
            self.run_effects(opening);
        }
    }

    /// Opens the new-document form, with the title ready for the keyboard.
    pub(crate) fn place_new(&mut self, section: &WritingSection, cx: &mut Context<Self>) {
        if self.places.of(section).form.is_some() {
            return;
        }
        let title = new_form_field(self.theme.palette(), cx);
        let folder = new_form_field(self.theme.palette(), cx);
        for field in [&title, &folder] {
            let section = section.clone();
            cx.subscribe(
                field,
                move |this, _, event: &ComposerEvent, cx| match event {
                    ComposerEvent::Submit => this.place_create(&section, cx),
                    ComposerEvent::Dismiss => this.place_close_form(&section, cx),
                    ComposerEvent::Attach => {}
                },
            )
            .detach();
        }
        self.focus_field_when_drawn(&title, cx);
        self.places.of_mut(section).form = Some(NewDocument { title, folder });
        cx.notify();
    }

    pub(crate) fn place_close_form(&mut self, section: &WritingSection, cx: &mut Context<Self>) {
        self.places.of_mut(section).form = None;
        self.field_request = None;
        cx.notify();
    }

    /// Makes the document the form describes. The form stays until the document opens, so that
    /// a title the engine refuses (one that exists already) can be changed.
    pub(crate) fn place_create(&mut self, section: &WritingSection, cx: &mut Context<Self>) {
        let (Some(workspace_id), Some(form)) = (
            self.state.selected_workspace_id(),
            &self.places.of(section).form,
        ) else {
            return;
        };
        let draft = DocumentDraft {
            title: form.title.read(cx).draft().text().to_owned(),
            folder: form.folder.read(cx).draft().text().to_owned(),
        };
        if !self.require_connection(cx) {
            return;
        }
        let effects = self.state.writing_create(&workspace_id, section, &draft);
        self.run_effects(effects);
        cx.notify();
    }
}
