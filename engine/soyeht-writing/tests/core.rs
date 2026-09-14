#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_protocol::{
    Actor, BeatCard, BeatCategory, FileAuthor, Role, ScreenplayElementKind, TextPosition,
    TextRange, WritingFormatKind, WritingSection, WritingVariant, limits,
};
use soyeht_writing::{
    BeatBoard, Error, HybridBlockKind, SectionEntryInput, VariantSet, advance_after,
    format_markdown, hybrid_blocks, new_document_path, parse_screenplay, section_entries,
    segment_slides,
};

fn position(line: u32, column: u32) -> TextPosition {
    TextPosition { line, column }
}

fn range(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> TextRange {
    TextRange {
        start: position(start_line, start_column),
        end: position(end_line, end_column),
    }
}

fn human(name: &str) -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role: Role::HumanLocal,
            client: "mac".to_owned(),
            device_id: None,
            pane_id: None,
        },
        name: name.to_owned(),
    }
}

fn agent(name: &str) -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role: Role::Agent,
            client: "engine".to_owned(),
            device_id: None,
            pane_id: Some("pane-agent".to_owned()),
        },
        name: name.to_owned(),
    }
}

#[test]
fn markdown_toolbar_emits_valid_markdown_for_every_closed_format() {
    let source = "Title\nalpha\nbeta\nlabel";
    let heading = format_markdown(
        source,
        &range(0, 0, 0, 5),
        WritingFormatKind::Heading2,
        None,
    )
    .unwrap();
    assert_eq!(heading, "## Title\nalpha\nbeta\nlabel");

    let bullet = format_markdown(
        source,
        &range(1, 0, 2, 4),
        WritingFormatKind::BulletList,
        None,
    )
    .unwrap();
    assert_eq!(bullet, "Title\n- alpha\n- beta\nlabel");

    let numbered = format_markdown(
        source,
        &range(1, 0, 2, 4),
        WritingFormatKind::NumberedList,
        None,
    )
    .unwrap();
    assert_eq!(numbered, "Title\n1. alpha\n2. beta\nlabel");

    let quote =
        format_markdown(source, &range(1, 0, 1, 5), WritingFormatKind::Quote, None).unwrap();
    assert_eq!(quote, "Title\n> alpha\nbeta\nlabel");

    let bold = format_markdown(source, &range(1, 0, 1, 5), WritingFormatKind::Bold, None).unwrap();
    assert_eq!(bold, "Title\n**alpha**\nbeta\nlabel");

    let italic =
        format_markdown(source, &range(1, 0, 1, 5), WritingFormatKind::Italic, None).unwrap();
    assert_eq!(italic, "Title\n_alpha_\nbeta\nlabel");

    let link = format_markdown(
        source,
        &range(3, 0, 3, 5),
        WritingFormatKind::Link,
        Some("https://example.test"),
    )
    .unwrap();
    assert_eq!(link, "Title\nalpha\nbeta\n[label](https://example.test)");

    let image = format_markdown(
        source,
        &range(3, 0, 3, 5),
        WritingFormatKind::Image,
        Some("images/cover.png"),
    )
    .unwrap();
    assert_eq!(image, "Title\nalpha\nbeta\n![label](images/cover.png)");
}

#[test]
fn new_document_paths_are_stable_nested_and_confined_to_their_section() {
    assert_eq!(
        new_document_path(
            &WritingSection::Texts,
            "  Field Notes!  ",
            Some("Research/2026")
        )
        .unwrap(),
        "Writing/Texts/Research/2026/field-notes.md"
    );
    assert_eq!(
        new_document_path(&WritingSection::Screenplays, "Night Train", None).unwrap(),
        "Writing/Screenplays/night-train.fountain"
    );
    assert_eq!(
        new_document_path(&WritingSection::Presentations, "Review", Some("../Texts")),
        Err(Error::InvalidSectionPath)
    );
    assert_eq!(
        new_document_path(&WritingSection::Texts, "***", None),
        Err(Error::InvalidSectionPath)
    );
}

#[test]
fn markdown_positions_are_utf16_and_never_split_a_scalar() {
    let source = "A😀B";
    let result =
        format_markdown(source, &range(0, 1, 0, 3), WritingFormatKind::Bold, None).unwrap();
    assert_eq!(result, "A**😀**B");
    assert_eq!(
        format_markdown(source, &range(0, 1, 0, 2), WritingFormatKind::Bold, None,),
        Err(Error::InvalidRange)
    );
}

#[test]
fn hybrid_preview_covers_rich_blocks_and_only_exposes_active_line_syntax() {
    let source = concat!(
        "# Heading\n",
        "Paragraph with **weight**.\n",
        "  - nested item\n",
        "> quoted\n",
        "| Name | Value |\n",
        "| --- | --- |\n",
        "| one | two |\n",
        "```rust\n",
        "let value = 1;\n",
        "```\n",
        "![cover](cover.png)"
    );
    let blocks = hybrid_blocks(source, 2).unwrap();
    let kinds: Vec<_> = blocks.iter().map(|block| block.kind).collect();
    assert_eq!(
        kinds,
        vec![
            HybridBlockKind::Heading,
            HybridBlockKind::Paragraph,
            HybridBlockKind::ListItem,
            HybridBlockKind::Quote,
            HybridBlockKind::Table,
            HybridBlockKind::Table,
            HybridBlockKind::Table,
            HybridBlockKind::CodeFence,
            HybridBlockKind::Code,
            HybridBlockKind::CodeFence,
            HybridBlockKind::Image,
        ]
    );
    assert_eq!(blocks.iter().filter(|block| block.raw_syntax).count(), 1);
    assert_eq!(blocks[0].text, "Heading");
    assert_eq!(blocks[2].text, "  - nested item");
    assert_eq!(blocks[3].text, "quoted");
    assert!(blocks[7].text.is_empty());
    assert_eq!(blocks[8].text, "let value = 1;");
}

#[test]
fn slide_segmentation_accepts_rule_or_three_returns_and_separates_notes() {
    let source = concat!(
        "# First slide\n",
        "speaker note\n",
        "\n",
        "\n",
        "# Second slide\n",
        "![diagram](images/diagram.png)\n",
        "---\n",
        "# Third slide\n",
        "closing note"
    );
    let slides = segment_slides(source).unwrap();
    assert_eq!(slides.len(), 3);
    assert_eq!(slides[0].heading, "First slide");
    assert_eq!(slides[0].notes, "speaker note");
    assert_eq!(slides[1].heading, "Second slide");
    assert_eq!(slides[1].image_paths, vec!["images/diagram.png"]);
    assert_eq!(slides[2].heading, "Third slide");
    assert_eq!(slides[2].notes, "closing note");
}

#[test]
fn slide_count_is_bounded() {
    let source = (0..=limits::WRITING_SLIDES)
        .map(|index| format!("# Slide {index}"))
        .collect::<Vec<_>>()
        .join("\n---\n");
    assert_eq!(segment_slides(&source), Err(Error::SlideLimit));
}

#[test]
fn fountain_parser_builds_ordered_elements_scenes_and_smart_type() {
    let source = concat!(
        "INT. KITCHEN - NIGHT\n",
        "A clock ticks.\n",
        "\n",
        "ARIA (V.O.)\n",
        "(quietly)\n",
        "What time is it?\n",
        "\n",
        "CUT TO:\n",
        "\n",
        "EXT. STATION - DAY\n",
        "A train arrives.\n"
    );
    let screenplay = parse_screenplay(source).unwrap();
    let kinds: Vec<_> = screenplay
        .elements
        .iter()
        .map(|element| element.kind.clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            ScreenplayElementKind::SceneHeading,
            ScreenplayElementKind::Action,
            ScreenplayElementKind::Character,
            ScreenplayElementKind::Parenthetical,
            ScreenplayElementKind::Dialogue,
            ScreenplayElementKind::Transition,
            ScreenplayElementKind::SceneHeading,
            ScreenplayElementKind::Action,
        ]
    );
    assert_eq!(screenplay.scenes.len(), 2);
    assert_eq!(screenplay.scenes[0].heading, "INT. KITCHEN - NIGHT");
    assert_eq!(
        screenplay.elements[4].scene_id.as_deref(),
        Some("scene-0-0")
    );
    assert_eq!(
        screenplay.elements[6].scene_id.as_deref(),
        Some("scene-1-9")
    );

    let smart = screenplay.smart_type();
    assert_eq!(smart.character_names, vec!["ARIA"]);
    assert_eq!(smart.locations, vec!["KITCHEN", "STATION"]);
    assert!(smart.transitions.contains(&"DISSOLVE TO:".to_owned()));
    assert!(smart.character_extensions.contains(&"(CONT'D)".to_owned()));
    assert_eq!(
        advance_after(&ScreenplayElementKind::Character, false),
        ScreenplayElementKind::Dialogue
    );
    assert_eq!(
        advance_after(&ScreenplayElementKind::Dialogue, true),
        ScreenplayElementKind::Parenthetical
    );
}

#[test]
fn line_variants_keep_one_canonical_choice_and_survive_restore() {
    let mut variants = VariantSet::new(
        "line-4".to_owned(),
        4,
        "What time is it?".to_owned(),
        human("Writer"),
        "variant-1".to_owned(),
    )
    .unwrap();
    variants
        .add(WritingVariant {
            variant_id: "variant-2".to_owned(),
            text: "Do you know the time?".to_owned(),
            author: agent("Aria"),
        })
        .unwrap();
    assert_eq!(
        variants.select("variant-2").unwrap(),
        "Do you know the time?"
    );
    let stored = variants.public();
    assert_eq!(stored.selected_index, 1);
    assert_eq!(
        VariantSet::restore(stored).unwrap().selected(),
        "Do you know the time?"
    );
    assert_eq!(variants.remove("variant-2").unwrap(), "What time is it?");
    assert_eq!(
        variants.remove("variant-1"),
        Err(Error::CannotRemoveOnlyVariant)
    );
}

#[test]
fn line_variant_count_and_payload_are_bounded() {
    let mut variants = VariantSet::new(
        "line-1".to_owned(),
        1,
        "one".to_owned(),
        human("Writer"),
        "v0".to_owned(),
    )
    .unwrap();
    for index in 1..limits::WRITING_VARIANTS_PER_LINE {
        variants
            .add(WritingVariant {
                variant_id: format!("v{index}"),
                text: format!("take {index}"),
                author: agent("Aria"),
            })
            .unwrap();
    }
    assert_eq!(
        variants.add(WritingVariant {
            variant_id: "overflow".to_owned(),
            text: "one more".to_owned(),
            author: agent("Aria"),
        }),
        Err(Error::VariantLimit)
    );
    assert_eq!(
        VariantSet::new(
            "line-2".to_owned(),
            2,
            "x".repeat(limits::WRITING_VARIANT_BYTES + 1),
            human("Writer"),
            "large".to_owned(),
        ),
        Err(Error::VariantTooLarge)
    );
}

fn beat(id: &str, order: u32, scene: Option<&str>) -> BeatCard {
    BeatCard {
        beat_id: id.to_owned(),
        title: format!("Beat {id}"),
        body: "A bounded idea".to_owned(),
        category: BeatCategory::Story,
        order,
        scene_id: scene.map(str::to_owned),
    }
}

#[test]
fn beat_board_links_both_directions_reorders_and_survives_reload() {
    let scenes = vec!["scene-a".to_owned(), "scene-b".to_owned()];
    let mut board = BeatBoard::restore(Vec::new(), &scenes).unwrap();
    board.put(beat("a", 0, Some("scene-a")), &scenes).unwrap();
    board.put(beat("b", 1, None), &scenes).unwrap();
    assert_eq!(board.beats_for_scene("scene-a")[0].beat_id, "a");
    board.move_to("b", 0).unwrap();
    assert_eq!(board.cards()[0].beat_id, "b");
    assert_eq!(board.cards()[1].order, 1);

    let restored = BeatBoard::restore(board.cards().to_vec(), &scenes).unwrap();
    assert_eq!(restored.cards(), board.cards());
    assert_eq!(
        board.put(beat("outside", 2, Some("scene-missing")), &scenes),
        Err(Error::SceneNotFound)
    );
}

fn entry(path: &str, title: &str, modified_at: &str) -> SectionEntryInput {
    SectionEntryInput {
        path: path.to_owned(),
        title: title.to_owned(),
        file_id: None,
        modified_at: modified_at.to_owned(),
        byte_count: 42,
    }
}

#[test]
fn writing_sections_are_bounded_nested_and_never_mix_types() {
    let texts = section_entries(
        WritingSection::Texts,
        vec![
            entry(
                "Writing/Texts/notes/second.md",
                "Second",
                "2026-09-13T12:00:00Z",
            ),
            entry("Writing/Texts/first.md", "First", "2026-09-14T12:00:00Z"),
        ],
    )
    .unwrap();
    assert_eq!(texts[0].title, "First");
    assert_eq!(texts[1].path, "Writing/Texts/notes/second.md");
    assert_eq!(texts[0].byte_count, "42");

    assert_eq!(
        section_entries(
            WritingSection::Texts,
            vec![entry(
                "Writing/Presentations/deck.md",
                "Wrong section",
                "2026-09-14T12:00:00Z",
            )],
        ),
        Err(Error::InvalidSectionPath)
    );
    assert_eq!(
        section_entries(
            WritingSection::Screenplays,
            vec![entry(
                "Writing/Screenplays/../Texts/escape.fountain",
                "Escape",
                "2026-09-14T12:00:00Z",
            )],
        ),
        Err(Error::InvalidSectionPath)
    );
}
