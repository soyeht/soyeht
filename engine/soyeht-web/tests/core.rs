#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_protocol::{
    AttachmentRef, WebAccessibility, WebCommandKind, WebCommandParams, WebComponentSource,
    WebComputedStyles, WebConsoleLevel, WebInteractionMode, WebPickedElement, WebRect,
    WebViewportPreset, limits,
};
use soyeht_web::{
    ConsoleMetadata, ConsoleRing, CwdObservation, ElementCapture, Error, LocalServerScan,
    NavigationReferences, NetworkMetadata, NetworkRing, NormalizedUrl, ServerTitle,
    SnapshotNodeInput, build_element_package, build_snapshot, command_response_budget,
    validate_command, viewport,
};

#[test]
fn urls_normalize_bare_hosts_and_keep_only_http_schemes() {
    let remote = NormalizedUrl::parse("example.com/a?q=1#part").unwrap();
    assert_eq!(remote.as_str(), "https://example.com/a?q=1#part");
    assert_eq!(remote.host(), "example.com");
    assert!(!remote.is_local());

    for input in ["localhost:5173", "127.0.0.1:3000", "[::1]:8080"] {
        let local = NormalizedUrl::parse(input).unwrap();
        assert!(local.as_str().starts_with("http://"));
        assert!(local.is_local());
    }
    assert!(
        NormalizedUrl::parse("https://localhost:5173/")
            .unwrap()
            .is_local()
    );
    assert_eq!(
        NormalizedUrl::parse("file:///tmp/index.html"),
        Err(Error::UnsupportedScheme)
    );
    assert_eq!(
        NormalizedUrl::parse("mailto:user@example.invalid"),
        Err(Error::UnsupportedScheme)
    );
    assert_eq!(
        NormalizedUrl::parse("https://user:secret@example.invalid/"),
        Err(Error::InvalidUrl)
    );
    assert_eq!(
        NormalizedUrl::parse("https://bad host/"),
        Err(Error::InvalidUrl)
    );
}

#[test]
fn local_server_scan_is_machine_parsed_sorted_deduplicated_and_bounded() {
    let output = b"p20\0cnode\0n*:5173\0n127.0.0.1:5173\0p10\0cpython\0n[::1]:3000\0";
    let scan = LocalServerScan::parse(
        output,
        &[
            CwdObservation {
                pid: 20,
                cwd: "/tmp/site-b".into(),
            },
            CwdObservation {
                pid: 10,
                cwd: "/tmp/site-a".into(),
            },
        ],
        &[ServerTitle {
            pid: 20,
            port: 5173,
            title: "Site B".into(),
        }],
    )
    .unwrap();
    assert_eq!(scan.servers().len(), 2);
    assert_eq!(scan.servers()[0].port, 3000);
    assert_eq!(scan.servers()[1].port, 5173);
    assert_eq!(scan.servers()[1].title.as_deref(), Some("Site B"));

    let many = (1..=70)
        .flat_map(|port| format!("p{port}\ncdev\nn*:{}\n", 10_000 + port).into_bytes())
        .collect::<Vec<_>>();
    let cwds = (1..=70)
        .map(|pid| CwdObservation {
            pid,
            cwd: format!("/tmp/{pid}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        LocalServerScan::parse(&many, &cwds, &[])
            .unwrap()
            .servers()
            .len(),
        limits::LOCAL_SERVER_ENTRIES
    );
}

#[test]
fn lsof_listener_and_cwd_records_are_parsed_by_the_pure_core() {
    let listeners = b"p20\0cnode\0f7\0n127.0.0.1:5173\0\np10\0cpython\0f8\0n*:3000\0\n";
    let directories = b"p10\0fcwd\0n/tmp/site-a\0\np20\0fcwd\0n/tmp/site-b\0\n";
    assert_eq!(LocalServerScan::listener_pids(listeners).unwrap(), [10, 20]);
    let scan = LocalServerScan::parse_lsof(listeners, directories, &[]).unwrap();
    assert_eq!(scan.servers()[0].cwd, "/tmp/site-a");
    assert_eq!(scan.servers()[1].cwd, "/tmp/site-b");
}

#[test]
fn local_server_scan_rejects_unbounded_or_malformed_observations() {
    assert_eq!(
        LocalServerScan::parse(&vec![b'x'; limits::PORT_SCAN_BYTES + 1], &[], &[]),
        Err(Error::PortScanTooLarge)
    );
    assert_eq!(
        LocalServerScan::parse(b"p0\ncnode\nn*:3000\n", &[], &[]),
        Err(Error::InvalidPortRecord)
    );
    assert_eq!(
        LocalServerScan::parse(
            b"p1\ncnode\nn*:3000\n",
            &[CwdObservation {
                pid: 1,
                cwd: "bad\npath".into(),
            }],
            &[],
        ),
        Err(Error::InvalidPortRecord)
    );
}

#[test]
fn navigation_references_expire_atomically() {
    let mut references = NavigationReferences::new(7);
    let first = references.issue().unwrap();
    assert_eq!(first, "e1");
    assert!(references.contains(7, &first).unwrap());
    references.begin_navigation(8);
    assert_eq!(references.contains(7, &first), Err(Error::StaleReference));
    assert!(!references.contains(8, &first).unwrap());
    assert_eq!(references.issue().unwrap(), "e1");
}

#[test]
fn snapshot_is_parent_ordered_and_truncates_before_its_byte_budget() {
    let mut references = NavigationReferences::new(1);
    let mut inputs = vec![node("button", "Save", None)];
    inputs.extend((0..400).map(|_| node("textbox", &"x".repeat(1500), Some(0))));
    let snapshot = build_snapshot(&mut references, inputs).unwrap();
    assert!(snapshot.truncated);
    assert_eq!(snapshot.nodes[0].r#ref, "e1");
    assert_eq!(snapshot.nodes[1].parent_ref.as_deref(), Some("e1"));
    assert!(serde_json::to_vec(&snapshot.nodes).unwrap().len() < limits::WEB_SNAPSHOT_BYTES);
    assert!(
        references
            .contains(1, &snapshot.nodes.last().unwrap().r#ref)
            .unwrap()
    );
    let next = build_snapshot(&mut references, [node("link", "Next", None)]).unwrap();
    assert_ne!(next.nodes[0].r#ref, "e1");
}

#[test]
fn snapshot_rejects_forward_parents_and_oversized_fields() {
    let mut references = NavigationReferences::new(1);
    assert_eq!(
        build_snapshot(&mut references, [node("button", "Save", Some(0))]),
        Err(Error::InvalidSnapshotNode)
    );
    assert_eq!(
        build_snapshot(&mut references, [node(&"r".repeat(257), "Save", None)]),
        Err(Error::InvalidSnapshotNode)
    );
}

#[test]
fn element_packages_enforce_selection_html_image_and_total_budgets() {
    let capture = element_capture(
        limits::WEB_OUTER_HTML_BYTES,
        limits::WEB_PICK_CROP_WIDTH as u32,
    );
    let package =
        build_element_package("localhost:5173", "Site".into(), vec![capture.clone()]).unwrap();
    assert_eq!(package.url, "http://localhost:5173/");
    assert_eq!(package.elements.len(), 1);
    assert_eq!(
        build_element_package(
            "https://example.invalid",
            "Site".into(),
            vec![capture.clone(); limits::WEB_PICKED_ELEMENTS + 1],
        ),
        Err(Error::ElementLimit)
    );
    assert_eq!(
        build_element_package(
            "https://example.invalid",
            "Site".into(),
            vec![element_capture(limits::WEB_OUTER_HTML_BYTES + 1, 1)],
        ),
        Err(Error::InvalidElement)
    );
    assert_eq!(
        build_element_package(
            "https://example.invalid",
            "Site".into(),
            vec![element_capture(1, limits::WEB_PICK_CROP_WIDTH as u32 + 1)],
        ),
        Err(Error::InvalidElement)
    );
}

#[test]
fn console_ring_evicts_whole_entries_and_reports_the_gap() {
    let mut ring = ConsoleRing::default();
    for index in 0..=limits::WEB_CONSOLE_ENTRIES {
        ring.push(ConsoleMetadata {
            level: WebConsoleLevel::Info,
            text: format!("entry {index}"),
            source: "console".into(),
            at: "now".into(),
        })
        .unwrap();
    }
    let page = ring.page(Some(0));
    assert!(page.truncated);
    assert_eq!(page.entries.len(), limits::WEB_CONSOLE_ENTRIES);
    assert_eq!(page.entries[0].sequence, "2");
    assert_eq!(page.next, limits::WEB_CONSOLE_ENTRIES as u64 + 1);

    let mut large = ConsoleRing::default();
    for _ in 0..limits::WEB_CONSOLE_ENTRIES {
        large
            .push(ConsoleMetadata {
                level: WebConsoleLevel::Info,
                text: "x".repeat(16_384),
                source: "console".into(),
                at: "now".into(),
            })
            .unwrap();
    }
    let bounded = large.page(None);
    assert!(bounded.truncated);
    assert!(bounded.entries.len() < limits::WEB_CONSOLE_ENTRIES);
    assert!(serde_json::to_vec(&bounded.entries).unwrap().len() < limits::JSON_BYTES);
}

#[test]
fn network_ring_exposes_only_bounded_metadata() {
    let mut ring = NetworkRing::default();
    for index in 0..=limits::WEB_NETWORK_ENTRIES {
        ring.push(NetworkMetadata {
            method: "GET".into(),
            url: format!("https://example.invalid/{index}"),
            status: 200,
            duration_ms: 3,
            bytes: 12,
            at: "now".into(),
        })
        .unwrap();
    }
    let page = ring.page(Some(1));
    assert!(!page.truncated);
    assert_eq!(page.entries.len(), limits::WEB_NETWORK_ENTRIES);
    let encoded = serde_json::to_value(&page.entries[0]).unwrap();
    assert!(encoded.get("headers").is_none());
    assert!(encoded.get("body").is_none());
    assert_eq!(
        ring.push(NetworkMetadata {
            method: String::new(),
            url: "https://example.invalid".into(),
            status: 200,
            duration_ms: 0,
            bytes: 0,
            at: "now".into(),
        }),
        Err(Error::InvalidNetworkEntry)
    );
}

#[test]
fn viewport_presets_and_rotation_are_closed_and_bounded() {
    let phone = viewport(WebViewportPreset::Iphone15Pro, None, None, false, 100).unwrap();
    assert_eq!((phone.width, phone.height), (393, 852));
    let rotated = viewport(WebViewportPreset::IphoneSe, None, None, true, 100).unwrap();
    assert_eq!((rotated.width, rotated.height), (667, 375));
    let responsive = viewport(
        WebViewportPreset::Responsive,
        Some(1200),
        Some(800),
        false,
        125,
    )
    .unwrap();
    assert_eq!((responsive.width, responsive.height), (1200, 800));
    assert_eq!(
        viewport(WebViewportPreset::Responsive, None, Some(800), false, 100),
        Err(Error::InvalidViewport)
    );
    assert_eq!(
        viewport(WebViewportPreset::Ipad, None, None, false, 501),
        Err(Error::InvalidViewport)
    );
}

#[test]
fn command_policy_disables_host_human_mode_and_bounds_wait_and_evaluate() {
    let wait = command(
        WebCommandKind::Wait,
        serde_json::json!({"text":"Ready"}),
        60_000,
    );
    assert_eq!(
        validate_command(&wait, WebInteractionMode::Structured),
        Ok(())
    );
    assert_eq!(
        validate_command(&wait, WebInteractionMode::Human),
        Err(Error::HumanModeUnavailable)
    );
    assert_eq!(
        validate_command(
            &command(WebCommandKind::Wait, serde_json::json!({}), 60_000),
            WebInteractionMode::Structured,
        ),
        Err(Error::InvalidWait)
    );
    assert_eq!(
        validate_command(
            &command(
                WebCommandKind::Evaluate,
                serde_json::json!({"js":"x".repeat(limits::WEB_EVALUATE_BYTES + 1)}),
                1_000,
            ),
            WebInteractionMode::Structured,
        ),
        Err(Error::PayloadTooLarge)
    );
}

#[test]
fn response_budgets_are_selected_by_closed_command_kind() {
    assert_eq!(command_response_budget(&WebCommandKind::Snapshot), 204_800);
    assert_eq!(
        command_response_budget(&WebCommandKind::Screenshot),
        2_097_152
    );
    assert_eq!(command_response_budget(&WebCommandKind::Evaluate), 65_536);
    assert_eq!(
        command_response_budget(&WebCommandKind::Click),
        limits::JSON_BYTES
    );
}

fn node(role: &str, name: &str, parent_index: Option<usize>) -> SnapshotNodeInput {
    SnapshotNodeInput {
        role: role.into(),
        name: name.into(),
        state: "enabled".into(),
        value: None,
        rect: WebRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
        parent_index,
    }
}

fn element_capture(html_bytes: usize, crop_width: u32) -> ElementCapture {
    ElementCapture {
        crop_width,
        element: WebPickedElement {
            selector: "#save".into(),
            path: vec!["html".into(), "body".into(), "button#save".into()],
            outer_html: "x".repeat(html_bytes),
            styles: WebComputedStyles {
                box_sizing: "border-box".into(),
                color: "black".into(),
                background: "none".into(),
                font: "16px sans-serif".into(),
                border: "none".into(),
                border_radius: "0".into(),
                display: "block".into(),
                position: "static".into(),
            },
            accessibility: WebAccessibility {
                role: "button".into(),
                name: "Save".into(),
                state: "enabled".into(),
            },
            viewport_rect: WebRect {
                x: 0,
                y: 0,
                width: 80,
                height: 30,
            },
            page_rect: WebRect {
                x: 0,
                y: 100,
                width: 80,
                height: 30,
            },
            image: AttachmentRef {
                attachment_id: "image-1".into(),
                mime_type: "image/png".into(),
                name: "element.png".into(),
                bytes: "4".into(),
            },
            component: Some(WebComponentSource {
                framework: "component".into(),
                component: "SaveButton".into(),
                source_file: Some("src/SaveButton.tsx".into()),
                line: Some(12),
            }),
        },
    }
}

fn command(command: WebCommandKind, args: serde_json::Value, timeout_ms: u32) -> WebCommandParams {
    WebCommandParams {
        pane_id: "pane-1".into(),
        command,
        args,
        timeout_ms,
        operation_key: "operation-1".into(),
    }
}
