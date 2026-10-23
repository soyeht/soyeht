//! What the window keeps for the writing places: for each text or screenplay pane the field that
//! holds its text, how far the pane is scrolled and the question that closing it may ask; for
//! each place that lists documents (Texts, Screenplays) the search line, the new-document form
//! and whether it is on the stage.

use crate::composer_field::ComposerField;
use gpui::{Entity, ScrollHandle};
use soyeht_client::writing::rules::Closing;
use soyeht_protocol::WritingSection;

/// A writing pane's own state; it hangs off the pane's `PaneUi`, so it goes when the pane closes.
#[derive(Default)]
pub(crate) struct WritingUi {
    /// The field that holds the text, made when first needed.
    pub field: Option<Entity<ComposerField>>,
    /// The scroll of the text, which follows the caret.
    pub scroll: ScrollHandle,
    /// Closing the pane waits for the person's answer: the document has edits not saved.
    pub closing: Option<Closing>,
}

/// The places that list the documents of a section, each with its own search and form.
#[derive(Default)]
pub(crate) struct Places {
    texts: PlaceUi,
    presentations: PlaceUi,
    screenplays: PlaceUi,
}

impl Places {
    pub(crate) fn of(&self, section: &WritingSection) -> &PlaceUi {
        match section {
            WritingSection::Texts => &self.texts,
            WritingSection::Presentations => &self.presentations,
            WritingSection::Screenplays => &self.screenplays,
        }
    }

    pub(crate) fn of_mut(&mut self, section: &WritingSection) -> &mut PlaceUi {
        match section {
            WritingSection::Texts => &mut self.texts,
            WritingSection::Presentations => &mut self.presentations,
            WritingSection::Screenplays => &mut self.screenplays,
        }
    }

    /// No place but `staying` is on the stage: coming back to one reads its list again.
    pub(crate) fn leave(&mut self, staying: Option<&WritingSection>) {
        for section in [
            WritingSection::Texts,
            WritingSection::Presentations,
            WritingSection::Screenplays,
        ] {
            if staying != Some(&section) {
                self.of_mut(&section).on_stage = false;
            }
        }
    }

    /// The form that has the keyboard to take, if a place has one.
    pub(crate) fn form_to_focus(&mut self) -> Option<&mut NewDocument> {
        [
            &mut self.texts,
            &mut self.presentations,
            &mut self.screenplays,
        ]
        .into_iter()
        .filter_map(|place| place.form.as_mut())
        .find(|form| form.focus)
    }
}

/// The place of one section.
#[derive(Default)]
pub(crate) struct PlaceUi {
    /// The line that narrows the list.
    pub search: Option<Entity<ComposerField>>,
    /// The fields of the new-document form, which is open while they exist.
    pub form: Option<NewDocument>,
    /// The place was on the stage in the last frame: coming back to it reads the list again.
    pub on_stage: bool,
    /// The place opens with the new-document form open.
    pub new_on_entry: bool,
}

/// The new-document form.
pub(crate) struct NewDocument {
    pub title: Entity<ComposerField>,
    pub folder: Entity<ComposerField>,
    /// The title takes the keyboard on the next frame.
    pub focus: bool,
}
