use super::*;

#[test]
fn beat_board_is_ordered_linked_idempotent_and_restart_durable() {
    let mut rig = Rig::new();
    let repository = rig.path.join("beat-board-repository");
    fs::create_dir_all(repository.join("Writing/Screenplays")).unwrap();
    fs::write(
        repository.join("Writing/Screenplays/episode-one.fountain"),
        "INT. KITCHEN - DAWN\n\nANA\nWhat time is it?\n\nEXT. STREET - MORNING\n\nThe city wakes.\n",
    )
    .unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Beat board",
        &repository,
        "beat-board-workspace",
    );
    let opened = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Screenplays/episode-one.fountain",
            "kind":"screenplay","idempotencyKey":"beat-board-open"}),
    );
    let file_id = opened["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let structure = client.call(
        "writing.structure",
        json!({"fileId":file_id,"revision":"1"}),
    );
    let first_scene = structure["scenes"][0]["sceneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_scene = structure["scenes"][1]["sceneId"]
        .as_str()
        .unwrap()
        .to_owned();

    let first = client.call(
        "writing.beat.put",
        json!({"fileId":file_id,"baseRevision":"1","title":"Opening image",
            "body":"Sunlight crosses the kitchen.","category":"setting","order":0,
            "sceneId":first_scene,"idempotencyKey":"beat-first"}),
    );
    let first_id = first["beat"]["beatId"].as_str().unwrap().to_owned();
    assert_eq!(first["document"]["sidecarRevision"], "1");
    let second = client.call(
        "writing.beat.put",
        json!({"fileId":file_id,"baseRevision":"1","title":"The question",
            "body":"Time becomes the episode theme.","category":"story","order":1,
            "sceneId":second_scene,"idempotencyKey":"beat-second"}),
    );
    let second_id = second["beat"]["beatId"].as_str().unwrap().to_owned();
    let unlinked_params = json!({"fileId":file_id,"baseRevision":"1","title":"Loose end",
        "body":"Decide where this belongs.","category":"research","order":2,
        "idempotencyKey":"beat-unlinked"});
    let unlinked = client.call("writing.beat.put", unlinked_params.clone());
    let unlinked_id = unlinked["beat"]["beatId"].as_str().unwrap().to_owned();
    assert_eq!(client.call("writing.beat.put", unlinked_params), unlinked);

    let move_params = json!({"fileId":file_id,"baseRevision":"1","beatId":first_id,
        "title":"Opening image","body":"Sunlight crosses the kitchen.",
        "category":"setting","order":2,"sceneId":first_scene,
        "idempotencyKey":"beat-move-first"});
    let moved = client.call("writing.beat.put", move_params.clone());
    assert_eq!(client.call("writing.beat.put", move_params), moved);
    let listed = client.call(
        "writing.beat.list",
        json!({"fileId":file_id,"revision":"1"}),
    );
    assert_eq!(listed["beats"][0]["beatId"], second_id);
    assert_eq!(listed["beats"][0]["order"], 0);
    assert_eq!(listed["beats"][1]["beatId"], unlinked_id);
    assert_eq!(listed["beats"][1]["order"], 1);
    assert_eq!(listed["beats"][2]["beatId"], first_id);
    assert_eq!(listed["beats"][2]["order"], 2);

    let linked = client.call(
        "writing.structure",
        json!({"fileId":file_id,"revision":"1"}),
    );
    assert_eq!(linked["scenes"][0]["linkedBeatIds"][0], first_id);
    assert_eq!(linked["scenes"][1]["linkedBeatIds"][0], second_id);
    let stale = client.request(
        "writing.beat.put",
        json!({"fileId":file_id,"baseRevision":"2","title":"Stale",
            "body":"Must not land.","category":"story","order":0,
            "idempotencyKey":"beat-stale"}),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut restored = rig.client();
    let after_restart = restored.call(
        "writing.beat.list",
        json!({"fileId":file_id,"revision":"1"}),
    );
    assert_eq!(after_restart, listed);
    let delete_params = json!({"fileId":file_id,"baseRevision":"1","beatId":unlinked_id,
        "idempotencyKey":"beat-delete-unlinked"});
    let deleted = restored.call("writing.beat.delete", delete_params.clone());
    assert!(deleted["beat"].is_null());
    assert_eq!(restored.call("writing.beat.delete", delete_params), deleted);
    let remaining = restored.call(
        "writing.beat.list",
        json!({"fileId":file_id,"revision":"1"}),
    );
    assert_eq!(remaining["beats"].as_array().unwrap().len(), 2);
}

#[test]
fn bound_agent_cannot_read_or_add_variants_in_another_workspace() {
    let rig = Rig::new();
    let first = rig.path.join("variant-agent-workspace");
    let second = rig.path.join("variant-document-workspace");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(second.join("Writing/Screenplays")).unwrap();
    fs::write(
        second.join("Writing/Screenplays/private.fountain"),
        "INT. ROOM - DAY\n\nANA\nKeep this private.\n",
    )
    .unwrap();
    let mut human = rig.client();
    let first_workspace =
        create_workspace(&mut human, "Variant agent", &first, "variant-agent-scope");
    let second_workspace = create_workspace(
        &mut human,
        "Variant document",
        &second,
        "variant-document-scope",
    );
    let file_id = human.call(
        "writing.document.open",
        json!({"workspaceId":second_workspace,"path":"Writing/Screenplays/private.fountain",
            "kind":"screenplay","idempotencyKey":"open-private-screenplay"}),
    )["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, pane) = rig.agent_in_workspace(&first_workspace);
    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut agent, _) = binding_hello(&rig.path.join("engine.sock"), &binding);
    for (id, method, params) in [
        (
            2,
            "writing.structure",
            json!({"fileId":file_id,"revision":"1"}),
        ),
        (
            3,
            "writing.variant.add",
            json!({"fileId":file_id,"lineId":"element-3","baseRevision":"1",
                "text":"Do not cross this boundary.","idempotencyKey":"cross-workspace-variant"}),
        ),
    ] {
        let denied = binding_request(&mut agent, id, method, params);
        assert_eq!(denied["error"]["data"]["kind"], "unauthorized");
    }
}

#[test]
fn presentation_image_is_content_addressed_recoverable_and_idempotent() {
    let mut rig = Rig::new();
    let repository = rig.path.join("presentation-image-repository");
    fs::create_dir_all(repository.join("Writing/Presentations")).unwrap();
    fs::write(
        repository.join("Writing/Presentations/review.md"),
        "# Review\n\nSlide image\n",
    )
    .unwrap();
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Presentation image",
        &repository,
        "presentation-image-workspace",
    );
    let opened = client.call(
        "writing.document.open",
        json!({"workspaceId":workspace,"path":"Writing/Presentations/review.md","kind":"presentation","idempotencyKey":"presentation-image-open"}),
    );
    let file_id = opened["document"]["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned();
    let attachment_id = "30415263-7485-496a-bbcc-ddeeff001122";
    let image = b"\x89PNG\r\n\x1a\npresentation-image";
    Frame::Attachment {
        header: soyeht_protocol::attachment::attachment_header(attachment_id).unwrap(),
        bytes: image.to_vec(),
    }
    .write_to(&mut client.socket)
    .unwrap();
    let params = json!({
        "fileId":file_id,"baseRevision":"1",
        "range":{"start":{"line":2,"column":0},"end":{"line":2,"column":11}},
        "kind":"image","image":{"attachmentId":attachment_id,"mimeType":"image/png",
            "name":"slide.png","bytes":image.len().to_string()},
        "idempotencyKey":"presentation-image-format"
    });
    let formatted = client.call("writing.format", params.clone());
    assert_eq!(formatted["file"]["revision"], "2");
    let digest = format!("{:x}", Sha256::digest(image));
    let asset = repository.join(format!("Writing/Presentations/assets/{digest}.png"));
    assert_eq!(fs::read(&asset).unwrap(), image);
    let read = client.call(
        "file.read",
        json!({"fileId":file_id,"fromLine":0,"lineCount":10}),
    );
    assert_eq!(
        read["lines"][2]["text"],
        format!("![Slide image](assets/{digest}.png)")
    );
    client.messages.clear();
    let asset_result = client.call(
        "writing.asset.read",
        json!({
            "fileId":file_id,"revision":"2",
            "asset":format!("assets/{digest}.png")
        }),
    );
    assert_eq!(asset_result["revision"], "2");
    let reference = &asset_result["image"];
    let header =
        soyeht_protocol::attachment::attachment_header(reference["attachmentId"].as_str().unwrap())
            .unwrap();
    let delivered: Vec<_> = client
        .messages
        .iter()
        .filter_map(|frame| match frame {
            Frame::Attachment {
                header: candidate,
                bytes,
            } if *candidate == header => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect();
    assert_eq!(delivered, image);
    assert_eq!(reference["bytes"], image.len().to_string());

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut retry = rig.client();
    Frame::Attachment {
        header: soyeht_protocol::attachment::attachment_header(attachment_id).unwrap(),
        bytes: image.to_vec(),
    }
    .write_to(&mut retry.socket)
    .unwrap();
    assert_eq!(retry.call("writing.format", params), formatted);
    assert_eq!(fs::read_dir(asset.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn runtime_rig_slot_keeps_owner_failure_visible_and_recovers_the_queue() {
    let failed_owner = thread::spawn(|| {
        let _runtime_slot = runtime_rig_slot();
        panic!("intentional runtime slot owner failure");
    })
    .join();

    assert!(
        failed_owner.is_err(),
        "the owner failure must stay observable"
    );
    drop(runtime_rig_slot());
}

#[test]
fn agent_messages_are_scoped_queued_labeled_and_rate_limited_per_direction() {
    let rig = Rig::new();
    let (mut human, sender) = rig.agent();
    let workspace = human.pane(&sender)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, target) = rig.agent_in_workspace(&workspace);
    human.call(
        "pane.draft",
        json!({"paneId":target,"active":true,"idempotencyKey":"hold-agent-messages"}),
    );
    let store = Store::open(&rig.path).unwrap();
    let runtime: Value = soyeht_store::agents::runtime(&store, &sender)
        .unwrap()
        .unwrap();
    let binding = runtime["binding"].as_str().unwrap();
    let (socket, hello) = binding_hello(&rig.path.join("engine.sock"), binding);
    assert_eq!(hello["result"]["role"], "agent");
    let mut agent = Client {
        socket,
        next: 1,
        messages: vec![],
    };
    let listed = agent.call("agent.list", json!({}));
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["paneId"] == target)
    );
    for index in 0..soyeht_protocol::limits::AGENT_MESSAGES_PER_WINDOW {
        let result = agent.call(
            "agent.message",
            json!({
                "targetPaneId":target,"text":format!("note {index}"),
                "idempotencyKey":format!("agent-note-{index}")
            }),
        );
        assert_eq!(result["queued"], true);
    }
    let blocked = agent.request(
        "agent.message",
        json!({
            "targetPaneId":target,"text":"seventh","idempotencyKey":"agent-note-seven"
        }),
    );
    assert_eq!(blocked["error"]["data"]["kind"], "rate_limited");
    assert!(blocked["error"]["data"]["retryAfterSec"].as_u64().unwrap() > 0);
    let queue = human.call("pane.queue.list", json!({"paneId":target,"limit":64}));
    let items = queue["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        soyeht_protocol::limits::AGENT_MESSAGES_PER_WINDOW
    );
    assert!(
        items
            .iter()
            .all(|item| item["agentMessage"]["paneId"] == sender)
    );
    // The queue holds the sender's words; the reply route is added only for
    // the harness, so no bubble or queue row shows a pane identifier.
    assert!(
        items
            .iter()
            .enumerate()
            .all(|(index, item)| item["text"] == format!("note {index}"))
    );
    assert!(
        items
            .iter()
            .all(|item| !item["text"].as_str().unwrap().contains("seventh"))
    );
    let other_workspace = create_workspace(&mut human, "Other", &rig.path, "other-agent-workspace");
    let (_, outsider) = rig.agent_in_workspace(&other_workspace);
    let forbidden = agent.request(
        "agent.message",
        json!({
            "targetPaneId":outsider,"text":"must stay local",
            "idempotencyKey":"cross-workspace-agent-message"
        }),
    );
    assert_eq!(forbidden["error"]["data"]["kind"], "not_found");
    let target_runtime: Value = soyeht_store::agents::runtime(&store, &target)
        .unwrap()
        .unwrap();
    let target_binding = target_runtime["binding"].as_str().unwrap();
    let (reply_socket, reply_hello) = binding_hello(&rig.path.join("engine.sock"), target_binding);
    assert_eq!(reply_hello["result"]["role"], "agent");
    let mut recipient = Client {
        socket: reply_socket,
        next: 1,
        messages: vec![],
    };
    assert_eq!(
        recipient.call(
            "agent.message",
            json!({
                "targetPaneId":sender,"text":"reply","idempotencyKey":"reverse-direction"
            })
        )["queued"],
        true
    );
}
