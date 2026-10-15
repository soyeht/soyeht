//! The cases of `contracts/vectors/attachments.json`, which the Mac client passes as well, and
//! the staging of real files and pictures.

use super::{image_prep as prep, *};
use crate::testing::{cases as listed, text, vector};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

fn cases(key: &str) -> Vec<Value> {
    listed("attachments", key)
}

fn kind(name: &str) -> PromptAttachmentKind {
    serde_json::from_value(json!(name)).unwrap()
}

#[test]
fn sizes_are_written_as_finder_writes_them() {
    for case in cases("fileSize") {
        let bytes = case["bytes"].as_u64().unwrap() as usize;
        assert_eq!(file_size(bytes), text(&case["text"]), "{bytes}");
    }
}

#[test]
fn a_chip_says_the_format_and_the_size() {
    for case in cases("detail") {
        let bytes = case["bytes"].as_u64().unwrap() as usize;
        assert_eq!(detail(text(&case["mimeType"]), bytes), text(&case["text"]));
    }
    for case in cases("glyph") {
        let glyph = Glyph::of(&kind(text(&case["kind"])), text(&case["mimeType"]));
        let name = format!("{glyph:?}").to_lowercase();
        assert_eq!(name, text(&case["glyph"]), "{case}");
    }
}

#[test]
fn files_are_sorted_by_their_name_as_the_mac_sorts_them() {
    for case in cases("fileKinds") {
        let (found, mime) = classify(text(&case["fileName"]));
        assert_eq!(found, kind(text(&case["kind"])), "{case}");
        assert_eq!(mime, text(&case["mimeType"]), "{case}");
    }
}

#[test]
fn a_paste_attaches_files_and_pictures_but_leaves_text_to_the_field() {
    for case in cases("pasteOffers") {
        let offer = PasteOffer {
            files: case["files"].as_u64().unwrap() as usize,
            image: case["image"].as_bool().unwrap(),
            text: case["text"].as_bool().unwrap(),
        };
        assert_eq!(
            offer.offers_attachments(),
            case["offers"].as_bool().unwrap(),
            "{case}"
        );
    }
}

#[test]
fn the_image_policy_is_the_shared_one() {
    let policy = vector("attachments")["imagePolicy"].clone();
    assert_eq!(policy["maximumPixels"], prep::IMAGE_MAXIMUM_PIXELS);
    assert_eq!(policy["maximumBytes"], prep::IMAGE_MAXIMUM_BYTES);
    assert_eq!(policy["thumbnailPixels"], prep::THUMBNAIL_PIXELS);
    assert_eq!(policy["fallbackName"], prep::IMAGE_FALLBACK_NAME);
    assert_eq!(policy["scalePixels"], json!(prep::SCALE_PIXELS));
    let percent = |values: &Value| -> Vec<u64> {
        values
            .as_array()
            .unwrap()
            .iter()
            .map(|quality| (quality.as_f64().unwrap() * 100.0).round() as u64)
            .collect()
    };
    let expected: Vec<u64> = prep::JPEG_QUALITIES.iter().map(|q| u64::from(*q)).collect();
    assert_eq!(percent(&policy["qualities"]), expected);
    let attempts: Vec<(u64, u64)> = policy["thumbnailAttempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attempt| {
            (
                attempt["pixels"].as_u64().unwrap(),
                (attempt["quality"].as_f64().unwrap() * 100.0).round() as u64,
            )
        })
        .collect();
    let held: Vec<(u64, u64)> = prep::THUMBNAIL_ATTEMPTS
        .iter()
        .map(|(pixels, quality)| (u64::from(*pixels), u64::from(*quality)))
        .collect();
    assert_eq!(attempts, held);
    for case in cases("keepsImage") {
        let kept = keeps_original(
            text(&case["mimeType"]),
            case["bytes"].as_u64().unwrap() as usize,
            case["width"].as_u64().unwrap() as u32,
            case["height"].as_u64().unwrap() as u32,
        );
        assert_eq!(kept, case["keeps"].as_bool().unwrap(), "{case}");
    }
    for case in cases("imageName") {
        let name = image_file_name(case["name"].as_str(), text(&case["sentAs"]));
        assert_eq!(name, text(&case["fileName"]), "{case}");
    }
}

fn draft_of(name: &str, size: usize, source: Source) -> Draft {
    Draft::new(
        PromptAttachmentKind::File,
        name.into(),
        "application/octet-stream".into(),
        size,
        source,
        None,
    )
}

fn bytes_draft(name: &str, size: usize) -> Draft {
    draft_of(name, size, Source::Bytes(vec![0; size]))
}

fn linked_draft(name: &str, size: usize) -> Draft {
    draft_of(name, size, Source::HostFile(format!("/tmp/{name}")))
}

#[test]
fn a_message_takes_at_most_the_engines_limits_and_says_why_not() {
    for case in cases("limits") {
        let mut list = List::default();
        let outcomes: Vec<String> = case["adds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|add| {
                let (name, size) = (text(&add["name"]), add["bytes"].as_u64().unwrap() as usize);
                let draft = if add["linked"].as_bool().unwrap() {
                    linked_draft(name, size)
                } else {
                    bytes_draft(name, size)
                };
                match list.add(draft) {
                    Ok(()) => "ok",
                    Err(Rejection::TooMany) => "tooMany",
                    Err(Rejection::TooLarge { .. }) => "tooLarge",
                    Err(Rejection::TotalTooLarge) => "totalTooLarge",
                    Err(Rejection::Empty { .. }) => "empty",
                    Err(Rejection::Unreadable { .. }) => "unreadable",
                    Err(Rejection::Folder { .. }) => "folder",
                }
                .to_owned()
            })
            .collect();
        let expected: Vec<String> = serde_json::from_value(case["outcomes"].clone()).unwrap();
        assert_eq!(outcomes, expected, "{}", case["name"]);
    }
    for case in cases("rejections") {
        let name = case["name"].as_str().unwrap_or_default().to_owned();
        let rejection = match text(&case["rejection"]) {
            "tooMany" => Rejection::TooMany,
            "tooLarge" => Rejection::TooLarge { name },
            "totalTooLarge" => Rejection::TotalTooLarge,
            "empty" => Rejection::Empty { name },
            "unreadable" => Rejection::Unreadable { name },
            _ => Rejection::Folder { name },
        };
        assert_eq!(rejection.message(), text(&case["message"]));
    }
}

#[test]
fn a_draft_describes_itself_to_the_engine() {
    for case in cases("wire") {
        let draft = &case["draft"];
        let source = match draft["source"]["path"].as_str() {
            Some(path) => Source::HostFile(path.to_owned()),
            None => Source::Bytes(STANDARD.decode(text(&draft["source"]["bytes"])).unwrap()),
        };
        let held = Draft {
            id: text(&draft["id"]).to_owned(),
            kind: kind(text(&draft["kind"])),
            name: text(&draft["name"]).to_owned(),
            mime_type: text(&draft["mimeType"]).to_owned(),
            byte_count: draft["byteCount"].as_u64().unwrap() as usize,
            source,
            thumbnail: draft["thumbnail"]
                .as_str()
                .map(|thumbnail| STANDARD.decode(thumbnail).unwrap()),
        };
        let (attachment, upload) = held.wire();
        assert_eq!(
            serde_json::to_value(&attachment).unwrap(),
            case["prompt"],
            "{case}"
        );
        match (&case["frame"], upload) {
            (Value::Null, None) => {}
            (frame, Some(upload)) => {
                let wanted = json!({
                    "attachmentId": upload.reference.attachment_id,
                    "mimeType": upload.reference.mime_type,
                    "name": upload.reference.name,
                    "bytes": upload.reference.bytes,
                    "data": STANDARD.encode(&upload.bytes),
                });
                assert_eq!(frame, &wanted, "{case}");
            }
            other => panic!("the frame does not match: {other:?}"),
        }
    }
}

fn picture(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x % 251) as u8, (y % 241) as u8, ((x * y) % 239) as u8, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut out, format)
        .unwrap();
    out.into_inner()
}

#[test]
fn a_small_picture_goes_as_it_is_with_a_preview() {
    let data = picture(64, 48, image::ImageFormat::Png);
    let draft = stage_image(&data, "image/png", Some("shot.png")).unwrap();
    assert_eq!(draft.name, "shot.png");
    assert_eq!(draft.mime_type, "image/png");
    assert_eq!(draft.source, Source::Bytes(data));
    assert!(
        draft
            .thumbnail
            .is_some_and(|thumbnail| !thumbnail.is_empty())
    );
}

#[test]
fn a_type_the_agents_do_not_take_or_a_large_picture_is_reencoded_to_fit() {
    let tiff = picture(100, 80, image::ImageFormat::Tiff);
    let draft = stage_image(&tiff, "image/tiff", Some("scan.tiff")).unwrap();
    assert!(matches!(
        draft.mime_type.as_str(),
        "image/png" | "image/jpeg"
    ));
    assert_eq!(
        draft.name,
        format!(
            "scan.{}",
            if draft.mime_type == "image/png" {
                "png"
            } else {
                "jpeg"
            }
        )
    );
    let large = picture(3_000, 1_500, image::ImageFormat::Png);
    let draft = stage_image(&large, "image/png", None).unwrap();
    let Source::Bytes(bytes) = &draft.source else {
        panic!("a picture is sent as bytes");
    };
    let sent = image::load_from_memory(bytes).unwrap();
    assert!(sent.width().max(sent.height()) <= prep::IMAGE_MAXIMUM_PIXELS);
    assert_eq!(
        draft.name,
        format!(
            "Pasted image.{}",
            if draft.mime_type == "image/png" {
                "png"
            } else {
                "jpeg"
            }
        )
    );
    assert_eq!(draft.byte_count, bytes.len());
    assert!(stage_image(b"not a picture", "image/png", None).is_none());
}

fn scratch(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("soyeht-attachments-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join(name)
}

#[test]
fn files_are_linked_on_the_engines_machine_and_sent_as_bytes_otherwise() {
    let pdf = scratch("Report Q3.pdf");
    std::fs::write(&pdf, b"%PDF-1.7").unwrap();
    let linked = stage_path(&pdf, true).unwrap();
    assert_eq!(linked.kind, PromptAttachmentKind::File);
    assert_eq!(linked.mime_type, "application/pdf");
    assert_eq!(linked.byte_count, 8);
    let Source::HostFile(path) = &linked.source else {
        panic!("a file is linked");
    };
    assert!(path.ends_with("Report Q3.pdf") && std::path::Path::new(path).is_absolute());
    let sent = stage_path(&pdf, false).unwrap();
    assert_eq!(sent.source, Source::Bytes(b"%PDF-1.7".to_vec()));
}

#[test]
fn a_folder_a_missing_file_and_an_image_are_each_told_what_they_are() {
    let folder = scratch("stuff");
    std::fs::create_dir_all(&folder).unwrap();
    assert_eq!(
        stage_path(&folder, true),
        Err(Rejection::Folder {
            name: "stuff".into()
        })
    );
    let missing = scratch("gone.txt");
    assert_eq!(
        stage_path(&missing, true),
        Err(Rejection::Unreadable {
            name: "gone.txt".into()
        })
    );
    let image = scratch("pic.png");
    std::fs::write(&image, picture(32, 32, image::ImageFormat::Png)).unwrap();
    let draft = stage_path(&image, true).unwrap();
    // An image is always sent as the bytes prepared for the agent.
    assert!(matches!(draft.source, Source::Bytes(_)) && draft.thumbnail.is_some());
}

#[test]
fn a_chip_reads_from_a_draft_and_from_a_sent_attachment() {
    let draft = bytes_draft("notes.md", 1_536);
    let chip = Chip::of_draft(&draft);
    assert_eq!(
        (chip.name.as_str(), chip.detail.as_str()),
        ("notes.md", "OCTET-STREAM \u{b7} 2 KB")
    );
    let (sent, _) = draft.wire();
    let chip = Chip::of_sent(&sent, 3);
    assert_eq!(chip.id, "3-notes.md");
    assert_eq!(chip.glyph, Glyph::File);
}

#[test]
fn a_refused_file_leaves_a_notice_that_the_next_attachment_or_removal_clears() {
    let mut staged = Staged::default();
    assert!(!staged.is_shown());
    staged.add(vec![
        Ok(bytes_draft("a.bin", 5)),
        Err(Rejection::Folder {
            name: "stuff".into(),
        }),
    ]);
    assert_eq!(staged.items().len(), 1);
    assert_eq!(
        staged.notice(),
        Some("stuff is a folder; attach the files inside it.")
    );
    staged.add(vec![Ok(bytes_draft("b.bin", 5))]);
    assert_eq!(staged.notice(), None);
    let first = staged.items()[0].id.clone();
    staged.add(vec![Err(Rejection::TooMany)]);
    staged.remove(&first);
    assert_eq!((staged.items().len(), staged.notice()), (1, None));
    assert_eq!(staged.take().len(), 1);
    assert!(!staged.is_shown());
}
