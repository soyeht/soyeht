//! Writing: the text panes and the Texts place. A text pane holds its document in a field; what
//! is typed reaches the engine as the one edit that turns the page into the field's text
//! (`soyeht_client::writing`), and the engine saves the draft shortly after the last edit. This
//! carries what the person does to the replica and what the replica says back to the field.

use crate::{
    composer_field::{ComposerEvent, ComposerField, ProseEvent, new_field},
    destination::Destination,
    workbench::Workbench,
    writing_style,
    writing_ui::NewDocument,
};
use gpui::{AppContext, Context, Entity, FocusHandle, Focusable, Window};
use soyeht_client::Effect;
use soyeht_client::writing::rules::{self, Closing, DocumentDraft};
use soyeht_protocol::{
    PaneMode, WorkspacePaneLayoutAxis, WritingFormatKind, WritingKind, WritingSection,
};

/// The section the desktop client serves.
pub(crate) const TEXTS: WritingSection = WritingSection::Texts;

impl Workbench {
    /// Whether the pane is a text, which the desktop client edits.
    pub(crate) fn is_text(&self, pane_id: &str) -> bool {
        self.state.pane(pane_id).is_some_and(|pane| {
            pane.mode == PaneMode::Writing && pane.writing_kind == Some(WritingKind::Text)
        })
    }

    /// The field that holds the text of a pane, made when first needed.
    pub(crate) fn writing_field(
        &mut self,
        cx: &mut Context<Self>,
        pane_id: &str,
    ) -> Entity<ComposerField> {
        let owner = pane_id.to_owned();
        let ui = self.panes.of(pane_id);
        if let Some(made) = ui.writing.field.clone() {
            return made;
        }
        let (palette, scroll) = (self.theme.palette(), ui.writing.scroll.clone());
        let field =
            cx.new(|cx| ComposerField::prose(palette, writing_style::highlights, scroll, cx));
        cx.subscribe(&field, move |this, _, event: &ProseEvent, cx| match event {
            ProseEvent::Edited => this.writing_edited(&owner, cx),
            ProseEvent::Format(kind) => this.format_selection(&owner, kind.clone(), cx),
            ProseEvent::Save => this.save_writing(&owner, cx),
        })
        .detach();
        self.panes.of(pane_id).writing.field = Some(field.clone());
        field
    }

    /// Where the keys of a text pane go: its field.
    pub(crate) fn writing_focus(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        self.is_text(pane_id)
            .then(|| self.writing_field(cx, pane_id).focus_handle(cx))
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
                self.state.problem = Some(reason.to_owned());
                self.sync_writing_field(pane_id, cx);
            }
        }
        cx.notify();
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
        }
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

    /// Waits for the engine to save the document: the draft is saved shortly after the last edit.
    fn save_writing(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.editors.save(pane_id);
        self.run_effects(effects);
        cx.notify();
    }

    /// The close button of a text: the pane closes at once, or asks first when the document has
    /// edits not saved.
    pub(crate) fn request_close_writing(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(file) = self.state.editors.file_of_pane(pane_id) else {
            self.close_pane(pane_id);
            return;
        };
        let name = rules::file_name(Some(&file.doc.path), pane_id);
        match rules::closing(&name, file.doc.dirty || file.has_unsent()) {
            Closing::Immediately => self.close_pane(pane_id),
            asked => self.panes.of(pane_id).writing.closing = Some(asked),
        }
        cx.notify();
    }

    /// The answer to the question closing asked.
    pub(crate) fn answer_close_writing(
        &mut self,
        pane_id: &str,
        close: bool,
        cx: &mut Context<Self>,
    ) {
        self.panes.of(pane_id).writing.closing = None;
        if close {
            self.close_pane(pane_id);
        }
        cx.notify();
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
            self.state.problem =
                Some("No agent is ready to open beside this text. Check Preferences.".to_owned());
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

    // The Texts place.

    /// The place is on the stage: the first time in a row of frames its list is read again and
    /// what was left in its search and its form is cleared, as the Mac's section view starts over.
    pub(crate) fn texts_stage(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.texts.on_stage, true) {
            return;
        }
        self.texts.form = None;
        self.texts.search = None;
        let effects = self.state.writing_list(workspace_id, &TEXTS, false);
        self.run_effects(effects);
        if std::mem::take(&mut self.texts.new_on_entry) {
            self.texts_new(cx);
        }
        cx.notify();
    }

    /// The "Text" entry of the new-pane menu: the Texts place, with the form that makes a text.
    pub(crate) fn open_new_text(&mut self, cx: &mut Context<Self>) {
        self.menus.close();
        if self.require_connection(cx) {
            self.texts.on_stage = false;
            self.texts.new_on_entry = true;
            self.select_destination(Destination::Texts, cx);
        }
    }

    /// The line that narrows the list, made when first needed.
    pub(crate) fn texts_search(&mut self, cx: &mut Context<Self>) -> Entity<ComposerField> {
        if let Some(search) = &self.texts.search {
            return search.clone();
        }
        let search = new_field(self.theme.palette(), cx);
        cx.subscribe(&search, |_, _, _: &ComposerEvent, cx| cx.notify())
            .detach();
        self.texts.search = Some(search.clone());
        search
    }

    pub(crate) fn texts_more(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.writing_list(workspace_id, &TEXTS, true);
        self.run_effects(effects);
        cx.notify();
    }

    pub(crate) fn texts_retry(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.writing_list(workspace_id, &TEXTS, false);
        self.run_effects(effects);
        cx.notify();
    }

    /// Opens the document of the list that was chosen.
    pub(crate) fn texts_open(&mut self, workspace_id: &str, path: &str, cx: &mut Context<Self>) {
        if self.require_connection(cx) {
            let opening = self.state.writing_open(workspace_id, &TEXTS, path);
            self.run_effects(opening);
        }
    }

    /// Opens the new-document form, with the title ready for the keyboard.
    pub(crate) fn texts_new(&mut self, cx: &mut Context<Self>) {
        if self.texts.form.is_some() {
            return;
        }
        let title = new_field(self.theme.palette(), cx);
        let folder = new_field(self.theme.palette(), cx);
        for field in [&title, &folder] {
            cx.subscribe(field, |this, _, event: &ComposerEvent, cx| match event {
                ComposerEvent::Submit => this.texts_create(cx),
                ComposerEvent::Dismiss => this.texts_close_form(cx),
                ComposerEvent::Attach => {}
            })
            .detach();
        }
        self.texts.form = Some(NewDocument {
            title,
            folder,
            focus: true,
        });
        cx.notify();
    }

    pub(crate) fn texts_close_form(&mut self, cx: &mut Context<Self>) {
        self.texts.form = None;
        cx.notify();
    }

    /// Gives the keyboard to the title of the form, once, after it opened.
    pub(crate) fn focus_texts_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.texts.form
            && std::mem::take(&mut form.focus)
        {
            window.focus(&form.title.focus_handle(cx), cx);
        }
    }

    /// Makes the document the form describes. The form stays until the document opens, so that
    /// a title the engine refuses (one that exists already) can be changed.
    pub(crate) fn texts_create(&mut self, cx: &mut Context<Self>) {
        let (Some(workspace_id), Some(form)) =
            (self.state.selected_workspace_id(), &self.texts.form)
        else {
            return;
        };
        let draft = DocumentDraft {
            title: form.title.read(cx).draft().text().to_owned(),
            folder: form.folder.read(cx).draft().text().to_owned(),
        };
        if !self.require_connection(cx) {
            return;
        }
        let effects = self.state.writing_create(&workspace_id, &TEXTS, &draft);
        self.run_effects(effects);
        cx.notify();
    }
}
