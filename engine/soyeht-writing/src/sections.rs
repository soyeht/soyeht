use crate::{Error, Result};
use soyeht_protocol::{WritingKind, WritingSection, WritingSectionEntry, limits};
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionEntryInput {
    pub path: String,
    pub title: String,
    pub file_id: Option<String>,
    pub modified_at: String,
    pub byte_count: u64,
}

pub fn folder_for(section: &WritingSection) -> &'static str {
    match section {
        WritingSection::Texts => "Writing/Texts",
        WritingSection::Presentations => "Writing/Presentations",
        WritingSection::Screenplays => "Writing/Screenplays",
    }
}

pub fn kind_for(section: &WritingSection) -> WritingKind {
    match section {
        WritingSection::Texts => WritingKind::Text,
        WritingSection::Presentations => WritingKind::Presentation,
        WritingSection::Screenplays => WritingKind::Screenplay,
    }
}

pub fn new_document_path(
    section: &WritingSection,
    title: &str,
    relative_folder: Option<&str>,
) -> Result<String> {
    if title.trim().is_empty() || title.len() > 512 {
        return Err(Error::InvalidSectionPath);
    }
    let kind = kind_for(section);
    let mut path = Path::new(folder_for(section)).to_path_buf();
    if let Some(folder) = relative_folder.filter(|folder| !folder.is_empty()) {
        let nested = Path::new(folder);
        if folder.len() > 2048
            || nested.is_absolute()
            || nested
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(Error::InvalidSectionPath);
        }
        path.push(nested);
    }
    let extension = match kind {
        WritingKind::Text | WritingKind::Presentation => "md",
        WritingKind::Screenplay => "fountain",
    };
    path.push(format!("{}.{}", filename_stem(title)?, extension));
    let path = path.to_str().ok_or(Error::InvalidSectionPath)?.to_owned();
    validate_path(&path, folder_for(section), &kind)?;
    Ok(path)
}

fn filename_stem(title: &str) -> Result<String> {
    let mut stem = String::new();
    let mut separator = false;
    for character in title.trim().chars() {
        if character.is_alphanumeric() {
            for lowered in character.to_lowercase() {
                if stem.len().saturating_add(lowered.len_utf8()) > 240 {
                    break;
                }
                stem.push(lowered);
            }
            separator = false;
        } else if !stem.is_empty() {
            separator = true;
        }
        if stem.len() >= 240 {
            break;
        }
        if separator && !stem.ends_with('-') {
            stem.push('-');
            separator = false;
        }
    }
    while stem.ends_with('-') {
        stem.pop();
    }
    if stem.is_empty() {
        return Err(Error::InvalidSectionPath);
    }
    Ok(stem)
}

pub fn section_entries(
    section: WritingSection,
    inputs: Vec<SectionEntryInput>,
) -> Result<Vec<WritingSectionEntry>> {
    if inputs.len() > limits::WRITING_SECTION_ENTRIES {
        return Err(Error::SectionLimit);
    }
    let folder = folder_for(&section);
    let kind = kind_for(&section);
    let mut entries = Vec::with_capacity(inputs.len());
    for input in inputs {
        validate_path(&input.path, folder, &kind)?;
        if input.title.trim().is_empty() || input.title.len() > 512 {
            return Err(Error::InvalidSectionPath);
        }
        entries.push(WritingSectionEntry {
            path: input.path,
            title: input.title,
            kind: kind.clone(),
            section: section.clone(),
            file_id: input.file_id,
            modified_at: input.modified_at,
            byte_count: input.byte_count.to_string(),
        });
    }
    entries.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then(left.title.cmp(&right.title))
            .then(left.path.cmp(&right.path))
    });
    Ok(entries)
}

fn validate_path(path: &str, folder: &str, kind: &WritingKind) -> Result<()> {
    if path.is_empty() || path.len() > 4096 {
        return Err(Error::InvalidSectionPath);
    }
    let parsed = Path::new(path);
    if parsed.is_absolute()
        || parsed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || !parsed.starts_with(folder)
        || parsed == Path::new(folder)
        || !extension_allowed(parsed, kind)
    {
        return Err(Error::InvalidSectionPath);
    }
    Ok(())
}

fn extension_allowed(path: &Path, kind: &WritingKind) -> bool {
    let extension = path.extension().and_then(|value| value.to_str());
    match kind {
        WritingKind::Text | WritingKind::Presentation => extension == Some("md"),
        WritingKind::Screenplay => extension == Some("fountain"),
    }
}
