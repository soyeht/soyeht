//! What the Mac and the desktop client both say and decide for a writing document, outside the
//! wire: how the sections are named, how counts read, what the new-document form submits, which
//! entries a search keeps, how a pane names its file, when closing asks first and what the
//! selection toolbar of a text offers. The cases are `contracts/vectors/writing.json`, which the
//! Mac passes too (`WritingVectorTests`).

pub use crate::pane_close::Closing;
use soyeht_protocol::{WritingFormatKind, WritingKind, WritingSection, WritingSectionEntry};

/// The name of a section on the wire, which is also how it is told apart in a list.
pub fn name(section: &WritingSection) -> &'static str {
    match section {
        WritingSection::Texts => "texts",
        WritingSection::Presentations => "presentations",
        WritingSection::Screenplays => "screenplays",
    }
}

/// The kind of document a section holds.
pub fn kind(section: &WritingSection) -> WritingKind {
    match section {
        WritingSection::Texts => WritingKind::Text,
        WritingSection::Presentations => WritingKind::Presentation,
        WritingSection::Screenplays => WritingKind::Screenplay,
    }
}

pub fn title(section: &WritingSection) -> &'static str {
    match section {
        WritingSection::Texts => "Texts",
        WritingSection::Presentations => "Presentations",
        WritingSection::Screenplays => "Screenplays",
    }
}

/// What one document of the section is called.
pub fn singular(section: &WritingSection) -> &'static str {
    match section {
        WritingSection::Texts => "text",
        WritingSection::Presentations => "presentation",
        WritingSection::Screenplays => "screenplay",
    }
}

/// The line under the section's title.
pub fn subtitle(section: &WritingSection) -> &'static str {
    match section {
        WritingSection::Texts => "Essays, notes, and long-form Markdown",
        WritingSection::Presentations => "Slide documents with speaker notes",
        WritingSection::Screenplays => "Fountain scripts and scene plans",
    }
}

/// The folder of the workspace that holds the section's documents.
pub fn folder(section: &WritingSection) -> &'static str {
    name(section)
}

/// What a count is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counted {
    Word,
    Scene,
    Slide,
}

/// A count as a pane's header says it: "1 word", "2 words".
pub fn count_label(count: u64, counted: Counted) -> String {
    let (one, many) = match counted {
        Counted::Word => ("word", "words"),
        Counted::Scene => ("scene", "scenes"),
        Counted::Slide => ("slide", "slides"),
    };
    format!("{count} {}", if count == 1 { one } else { many })
}

/// What a section's list says when it shows no document: the heading, and under it where
/// documents are kept (not for a search that found nothing).
pub fn empty_section(section: &WritingSection, searching: bool) -> (&'static str, Option<String>) {
    if searching {
        return ("No matching documents", None);
    }
    (
        "No documents yet",
        Some(format!(
            "Create one here and it stays in the {} folder.",
            folder(section)
        )),
    )
}

/// The title of the form that makes a document of the section.
pub fn new_document_title(section: &WritingSection) -> String {
    format!("New {}", singular(section))
}

/// The line under the new-document form's fields.
pub fn new_document_note(section: &WritingSection) -> String {
    format!(
        "The document is created inside the {} folder.",
        folder(section)
    )
}

/// What the new-document form holds: a title and an optional subfolder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentDraft {
    pub title: String,
    pub folder: String,
}

/// What the new-document form asks the engine to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub title: String,
    pub relative_folder: Option<String>,
}

impl DocumentDraft {
    /// What to create, or `None` while the title is blank or a creation is already running, so
    /// that neither the button nor Return can create a blank or a second document.
    pub fn submission(&self, creating: bool) -> Option<Submission> {
        let title = self.title.trim();
        if creating || title.is_empty() {
            return None;
        }
        let folder = self.folder.trim();
        Some(Submission {
            title: title.to_owned(),
            relative_folder: (!folder.is_empty()).then(|| folder.to_owned()),
        })
    }
}

/// The entries whose title or path contain the trimmed `query`, ignoring case; all of them for a
/// blank query.
pub fn search<'a>(entries: &'a [WritingSectionEntry], query: &str) -> Vec<&'a WritingSectionEntry> {
    let needle = query.trim().to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            needle.is_empty()
                || entry.title.to_lowercase().contains(&needle)
                || entry.path.to_lowercase().contains(&needle)
        })
        .collect()
}

/// The name a pane's header shows: the last part of the document's path, else the pane's own
/// name while it has no document yet.
pub fn file_name(path: Option<&str>, fallback: &str) -> String {
    path.map_or_else(
        || fallback.to_owned(),
        |path| path.rsplit('/').next().unwrap_or(path).to_owned(),
    )
}

/// Whether closing a pane with this file asks first: a document with edits not saved does.
pub fn closing(file_name: &str, dirty: bool) -> Closing {
    if dirty {
        Closing::asking(
            format!("Close {file_name}?"),
            "Unsaved changes will be lost.",
        )
    } else {
        Closing::Immediately
    }
}

/// The key, with the platform's command key, that applies a format to the selection.
pub fn format_shortcut(key: &str, command: bool) -> Option<WritingFormatKind> {
    if !command {
        return None;
    }
    match key.to_lowercase().as_str() {
        "b" => Some(WritingFormatKind::Bold),
        "i" => Some(WritingFormatKind::Italic),
        _ => None,
    }
}

/// A button of the toolbar over a selected part of a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    Bold,
    Italic,
    Heading,
    List,
    Quote,
    Link,
    Image,
}

impl SelectionAction {
    /// The name the vector gives the action.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Heading => "heading",
            Self::List => "list",
            Self::Quote => "quote",
            Self::Link => "link",
            Self::Image => "image",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::Heading => "Heading",
            Self::List => "List",
            Self::Quote => "Quote",
            Self::Link => "Link",
            Self::Image => "Image",
        }
    }

    /// The formats the action applies, each with its name: one for an action that applies at
    /// once, several for one that opens a menu of them.
    pub fn formats(self) -> Vec<(&'static str, WritingFormatKind)> {
        match self {
            Self::Bold => vec![("Bold", WritingFormatKind::Bold)],
            Self::Italic => vec![("Italic", WritingFormatKind::Italic)],
            Self::Heading => vec![
                ("Heading 1", WritingFormatKind::Heading1),
                ("Heading 2", WritingFormatKind::Heading2),
                ("Heading 3", WritingFormatKind::Heading3),
            ],
            Self::List => vec![
                ("Bulleted list", WritingFormatKind::BulletList),
                ("Numbered list", WritingFormatKind::NumberedList),
            ],
            Self::Quote => vec![("Quote", WritingFormatKind::Quote)],
            Self::Link => vec![("Link", WritingFormatKind::Link)],
            Self::Image => vec![("Image", WritingFormatKind::Image)],
        }
    }

    /// Whether the action asks for an address or a path before it applies.
    pub fn asks_destination(self) -> bool {
        matches!(self, Self::Link | Self::Image)
    }
}

/// The buttons over a selected part of a text; none without a selection.
pub fn selection_actions(has_selection: bool) -> &'static [SelectionAction] {
    const ALL: [SelectionAction; 7] = [
        SelectionAction::Bold,
        SelectionAction::Italic,
        SelectionAction::Heading,
        SelectionAction::List,
        SelectionAction::Quote,
        SelectionAction::Link,
        SelectionAction::Image,
    ];
    if has_selection { &ALL } else { &[] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, text, vector};
    use serde_json::{Value, json};

    fn section(case: &Value) -> WritingSection {
        serde_json::from_value(case["section"].clone()).unwrap()
    }

    #[test]
    fn the_sections_are_named_as_the_vector_says() {
        for case in cases("writing", "sections") {
            let section = section(&case);
            assert_eq!(title(&section), text(&case["title"]));
            assert_eq!(singular(&section), text(&case["singular"]));
            assert_eq!(subtitle(&section), text(&case["subtitle"]));
            assert_eq!(folder(&section), text(&case["folder"]));
            assert_eq!(name(&section), text(&case["section"]));
            assert_eq!(serde_json::to_value(kind(&section)).unwrap(), case["kind"]);
        }
    }

    #[test]
    fn a_section_speaks_as_the_vector_says() {
        for case in cases("writing", "sectionCopy") {
            let (heading, detail) =
                empty_section(&section(&case), case["searching"].as_bool().unwrap());
            assert_eq!(heading, text(&case["emptyHeading"]), "{}", case["name"]);
            assert_eq!(
                detail.as_deref(),
                case["emptyDetail"].as_str(),
                "{}",
                case["name"]
            );
        }
        for case in cases("writing", "newDocumentForm") {
            let section = section(&case);
            assert_eq!(new_document_title(&section), text(&case["title"]));
            assert_eq!(new_document_note(&section), text(&case["note"]));
        }
    }

    #[test]
    fn counts_read_as_the_vector_says() {
        for case in cases("writing", "counts") {
            let counted = match text(&case["kind"]) {
                "word" => Counted::Word,
                "scene" => Counted::Scene,
                _ => Counted::Slide,
            };
            assert_eq!(
                count_label(case["count"].as_u64().unwrap(), counted),
                text(&case["label"])
            );
        }
    }

    #[test]
    fn the_new_document_form_submits_as_the_vector_says() {
        for case in cases("writing", "documentDrafts") {
            let draft = DocumentDraft {
                title: text(&case["title"]).to_owned(),
                folder: text(&case["folder"]).to_owned(),
            };
            let expected = case["submission"].as_object().map(|submission| Submission {
                title: text(&submission["title"]).to_owned(),
                relative_folder: submission["relativeFolder"].as_str().map(str::to_owned),
            });
            assert_eq!(
                draft.submission(case["creating"].as_bool().unwrap()),
                expected,
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn the_search_keeps_the_entries_the_vector_says() {
        let filter = &vector("writing")["sectionFilter"];
        let entries: Vec<WritingSectionEntry> = filter["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                serde_json::from_value(json!({
                    "path": entry["path"], "title": entry["title"], "kind": "text",
                    "section": "texts", "modifiedAt": "", "byteCount": "0"
                }))
                .unwrap()
            })
            .collect();
        for case in filter["cases"].as_array().unwrap() {
            let kept: Vec<&str> = search(&entries, text(&case["query"]))
                .iter()
                .map(|entry| entry.path.as_str())
                .collect();
            let expected: Vec<&str> = case["paths"].as_array().unwrap().iter().map(text).collect();
            assert_eq!(kept, expected, "{}", case["name"]);
        }
    }

    #[test]
    fn a_pane_names_its_file_as_the_vector_says() {
        for case in cases("writing", "fileNames") {
            assert_eq!(
                file_name(case["path"].as_str(), text(&case["fallback"])),
                text(&case["fileName"]),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn closing_asks_as_the_vector_says() {
        for case in cases("writing", "closing") {
            let expected = case["confirm"]
                .as_object()
                .map_or(Closing::Immediately, |c| {
                    Closing::asking(text(&c["title"]), text(&c["message"]))
                });
            assert_eq!(
                closing(text(&case["fileName"]), case["dirty"].as_bool().unwrap()),
                expected,
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn the_shortcuts_are_the_ones_the_vector_says() {
        for case in cases("writing", "formatShortcuts") {
            let expected = case["format"]
                .as_str()
                .map(|format| serde_json::from_value(json!(format)).unwrap());
            assert_eq!(
                format_shortcut(text(&case["key"]), case["command"].as_bool().unwrap()),
                expected,
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn the_selection_toolbar_offers_what_the_vector_says() {
        let toolbar = &vector("writing")["selectionToolbar"];
        let expected = toolbar["text"].as_array().unwrap();
        let offered = selection_actions(true);
        assert_eq!(offered.len(), expected.len());
        for (action, case) in offered.iter().zip(expected) {
            assert_eq!(action.name(), text(&case["action"]));
            let formats: Vec<Value> = action
                .formats()
                .iter()
                .map(|(_, kind)| serde_json::to_value(kind).unwrap())
                .collect();
            assert_eq!(
                &Value::Array(formats),
                &case["formats"],
                "{}",
                action.name()
            );
            assert_eq!(
                action.asks_destination(),
                case["asksDestination"].as_bool().unwrap()
            );
        }
        assert_eq!(
            selection_actions(false).len(),
            toolbar["withoutSelection"].as_array().unwrap().len()
        );
    }
}
