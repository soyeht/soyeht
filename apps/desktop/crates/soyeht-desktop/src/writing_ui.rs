//! What the window keeps for the writing places: for each text pane the field that holds its
//! text, how far the pane is scrolled and the question that closing it may ask; for the Texts
//! place the search line, the new-document form and whether it is on the stage.

use crate::composer_field::ComposerField;
use gpui::{Entity, ScrollHandle};
use soyeht_client::writing::rules::Closing;

/// A text pane's own state; it hangs off the pane's `PaneUi`, so it goes when the pane closes.
#[derive(Default)]
pub(crate) struct WritingUi {
    /// The field that holds the text, made when first needed.
    pub field: Option<Entity<ComposerField>>,
    /// The scroll of the text, which follows the caret.
    pub scroll: ScrollHandle,
    /// Closing the pane waits for the person's answer: the document has edits not saved.
    pub closing: Option<Closing>,
}

/// The Texts place.
#[derive(Default)]
pub(crate) struct TextsUi {
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
