//! The avatar library through the engine's own dispatch (ADR 0166, contract 19).

use super::*;
use crate::{outbox, test_panes};
use soyeht_protocol::{Actor, ErrorKind, Role, attachment::attachment_header};

type Answer = std::result::Result<Outcome, errors::Error>;

struct Rig {
    core: Core,
    directory: PathBuf,
    local: String,
}

/// A PNG header of the size given, with a few bytes after it: what the library checks, and no image.
fn png_of(width: u32, height: u32, extra: u8) -> Vec<u8> {
    let mut bytes = png::header_of(width, height);
    bytes.extend_from_slice(&[extra; 16]);
    bytes
}

fn png(extra: u8) -> Vec<u8> {
    png_of(64, 48, extra)
}

/// A core on `directory`, whose background work runs at once.
fn boot(directory: &Path) -> Core {
    let mut core = Core::open(directory).unwrap();
    core.background = crate::offload::Background::inline();
    core.link_supervisor(Box::new(test_panes::Answering(|_| {
        Ok(soyeht_ptyd::wire::Response::Ok)
    })));
    core
}

impl Rig {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("soyeht-avatars-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let mut rig = Self {
            core: boot(&directory),
            directory,
            local: String::new(),
        };
        rig.local = rig.connect(Role::HumanLocal, None);
        rig
    }

    /// The engine stops and starts again on the same data.
    fn restart(&mut self) {
        let local = self.local.clone();
        self.core.disconnect(&local).unwrap();
        self.core = boot(&self.directory);
        self.local = self.connect(Role::HumanLocal, None);
    }

    /// A connection that has said `hello`: the person at the computer, or another role.
    fn connect(&mut self, role: Role, device: Option<&str>) -> String {
        let (outbox, receiver) = outbox::channel();
        // The test never reads this connection's notifications.
        std::mem::forget(receiver);
        let connection = self.core.connect(true, true, outbox).unwrap();
        if role == Role::HumanLocal {
            let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
                "client":"avatar-test","clientVersion":"1","protocolVersion":1}});
            self.core.dispatch(&connection, hello).unwrap();
            return connection;
        }
        let client = self.core.clients.get_mut(&connection).unwrap();
        client.scope = format!("device:{}", device.unwrap_or("test"));
        client.actor = Some(Actor {
            role,
            client: "test".into(),
            device_id: device.map(str::to_owned),
            pane_id: None,
        });
        connection
    }

    fn outcome(&mut self, connection: &str, method: &str, mut params: Value) -> Answer {
        if params.get("idempotencyKey").is_none() && method != "avatar.list" {
            params["idempotencyKey"] = json!(Uuid::new_v4().to_string());
        }
        let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        self.core.dispatch_inline(connection, request)
    }

    fn call_as(&mut self, connection: &str, method: &str, params: Value) -> Result<Value> {
        match self.outcome(connection, method, params)? {
            Outcome::Reply(value) => {
                soyeht_protocol::validate::result(method, &value).unwrap();
                Ok(value)
            }
            _ => panic!("{method} did not reply with a value"),
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let connection = self.local.clone();
        self.call_as(&connection, method, params)
    }

    /// Sends the bytes as the frames that come before a request, and names them as the request does.
    fn upload(&mut self, bytes: &[u8]) -> Value {
        let id = Uuid::new_v4().to_string();
        let header = attachment_header(&id).unwrap();
        let local = self.local.clone();
        let attachments = &mut self.core.clients.get_mut(&local).unwrap().attachments;
        for chunk in bytes.chunks(limits::ATTACHMENT_PAYLOAD_BYTES) {
            attachments.push(header, chunk).unwrap();
        }
        json!({"attachmentId": id, "mimeType": "image/png", "name": "face.png",
               "bytes": bytes.len().to_string()})
    }

    fn create(&mut self, name: &str, bytes: &[u8]) -> Result<Value> {
        let picture = self.upload(bytes);
        self.call(
            "avatar.create",
            json!({"name": name, "kind": "uploaded", "picture": picture}),
        )
    }

    fn avatar(&mut self, name: &str) -> Avatar {
        let made = self.create(name, &png(1)).unwrap();
        serde_json::from_value(made["avatar"].clone()).unwrap()
    }

    fn workspace(&mut self, name: &str) -> String {
        let path = self.directory.join(name);
        std::fs::create_dir(&path).unwrap();
        let made = self
            .call(
                "workspace.create",
                json!({"name": name, "path": path.to_string_lossy()}),
            )
            .unwrap();
        made["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn assign_as(
        &mut self,
        connection: &str,
        workspace: &str,
        avatar: &str,
        base: &str,
        extra: Value,
    ) -> Result<Value> {
        let mut params = json!({"workspaceId": workspace, "avatarId": avatar,
            "harness": "claude-code", "baseRevision": base});
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        self.call_as(connection, "avatar.assign", params)
    }

    fn assign(&mut self, workspace: &str, avatar: &str, base: &str, extra: Value) -> Result<Value> {
        let connection = self.local.clone();
        self.assign_as(&connection, workspace, avatar, base, extra)
    }

    /// The `avatar.changed` events written so far, as their parameters.
    fn changes(&mut self) -> Vec<Value> {
        self.core
            .store
            .replay(0)
            .unwrap()
            .1
            .into_iter()
            .filter(|event| event.method == "avatar.changed")
            .map(|event| {
                // What the engine publishes is what the contract says it is.
                let shown =
                    soyeht_protocol::contract::roundtrip("AvatarChangedEvent", &event.params);
                assert_eq!(shown.unwrap(), event.params);
                event.params
            })
            .collect()
    }

    fn folder(&self, avatar: &str) -> PathBuf {
        avatar_directory(&self.core, avatar)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn kind(answer: &Result<Value>) -> ErrorKind {
    errors::kind(answer.as_ref().unwrap_err())
}

#[test]
fn an_avatar_is_made_from_a_picture_and_is_listed() {
    let mut rig = Rig::new();
    let picture = rig.upload(&png(1));
    let made = rig
        .call(
            "avatar.create",
            json!({"name": "  Sabio ", "kind": "generated", "picture": picture,
                   "imageProvider": "a-provider", "imageModel": "a-model"}),
        )
        .unwrap();
    let avatar: Avatar = serde_json::from_value(made["avatar"].clone()).unwrap();
    assert_eq!(avatar.name, "Sabio");
    assert_eq!(avatar.source_hash, hex_digest(&png(1)));
    assert_eq!(avatar.image_provider.as_deref(), Some("a-provider"));
    assert_eq!(avatar.image_model.as_deref(), Some("a-model"));
    assert_eq!(
        std::fs::read(rig.folder(&avatar.avatar_id).join(SOURCE_FILE)).unwrap(),
        png(1)
    );
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["avatars"][0]["avatarId"], avatar.avatar_id);
    assert_eq!(listed["bindings"], json!([]));
    let events = rig.changes();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["avatar"]["name"], "Sabio");
    assert!(events[0].get("binding").is_none());
}

#[test]
fn a_picture_that_is_not_a_png_of_a_size_the_library_takes_makes_nothing() {
    let mut rig = Rig::new();
    let side = u32::try_from(limits::AVATAR_PICTURE_DIMENSION).unwrap();
    let signature_only = b"\x89PNG\r\n\x1a\n\0".to_vec();
    for (label, bytes) in [
        ("a GIF", b"GIF89a not a png".to_vec()),
        ("the signature and nothing else", signature_only),
        ("no width", png_of(0, 48, 1)),
        ("no height", png_of(64, 0, 1)),
        ("a width over the limit", png_of(side + 1, 48, 1)),
        ("a height over the limit", png_of(64, side + 1, 1)),
    ] {
        let refused = rig.create("Sabio", &bytes);
        assert_eq!(kind(&refused), ErrorKind::InvalidParams, "{label}");
    }
    let mut jpeg = rig.upload(&png(1));
    jpeg["mimeType"] = json!("image/jpeg");
    let refused = rig.call(
        "avatar.create",
        json!({"name": "Sabio", "kind": "uploaded", "picture": jpeg}),
    );
    assert_eq!(kind(&refused), ErrorKind::InvalidParams);
    assert_eq!(kind(&rig.create("   ", &png(1))), ErrorKind::InvalidParams);
    for refused_kind in ["preset", "hologram"] {
        let picture = rig.upload(&png(1));
        let refused = rig.call(
            "avatar.create",
            json!({"name": "Sabio", "kind": refused_kind, "picture": picture}),
        );
        assert_eq!(kind(&refused), ErrorKind::InvalidParams, "{refused_kind}");
    }
    assert_eq!(
        rig.call("avatar.list", json!({})).unwrap()["avatars"],
        json!([])
    );
    assert!(rig.changes().is_empty());
    let leftovers = std::fs::read_dir(rig.directory.join(AVATARS_DIRECTORY))
        .map_or(0, |entries| entries.count());
    assert_eq!(leftovers, 0);
}

#[test]
fn a_picture_that_never_arrived_or_is_too_large_is_refused_with_the_kind_the_contract_names() {
    let mut rig = Rig::new();
    // Named in the request, and never sent.
    let missing = json!({"attachmentId": Uuid::new_v4().to_string(), "mimeType": "image/png",
                         "name": "face.png", "bytes": "64"});
    let never = rig.call(
        "avatar.create",
        json!({"name": "Sabio", "kind": "uploaded", "picture": missing}),
    );
    assert_eq!(kind(&never), ErrorKind::InvalidParams);
    // Sent, but not what the request says it is.
    let mut short = rig.upload(&png(1));
    short["bytes"] = json!("999");
    let wrong_size = rig.call(
        "avatar.create",
        json!({"name": "Sabio", "kind": "uploaded", "picture": short}),
    );
    assert_eq!(kind(&wrong_size), ErrorKind::InvalidParams);
    // Over the library's limit, and under the connection's.
    let mut big = png(1);
    big.resize(limits::AVATAR_PICTURE_BYTES + 1, 0);
    assert_eq!(
        kind(&rig.create("Big", &big)),
        ErrorKind::AttachmentLimitExceeded
    );
    // Over what the connection takes at all.
    let mut huge = png(1);
    huge.resize(limits::PROMPT_ATTACHMENT_TOTAL_BYTES + 1, 0);
    assert_eq!(
        kind(&rig.create("Huge", &huge)),
        ErrorKind::AttachmentLimitExceeded
    );
    assert_eq!(
        rig.call("avatar.list", json!({})).unwrap()["avatars"],
        json!([])
    );
}

#[test]
fn a_request_the_schema_refuses_after_its_frames_were_sent_leaves_nothing_pending() {
    let mut rig = Rig::new();
    // An avatar.create refused by the schema (a name past the limit).
    let picture = rig.upload(&png(1));
    let long_name = "x".repeat(limits::AVATAR_NAME_CHARS + 1);
    let refused = rig.call(
        "avatar.create",
        json!({"name": long_name, "kind": "uploaded", "picture": picture}),
    );
    assert_eq!(kind(&refused), ErrorKind::InvalidParams);
    assert!(rig.call("avatar.list", json!({})).is_ok());
    // The same for the other users of attachment frames: a prompt and a writing image.
    for method in ["pane.prompt", "writing.format"] {
        rig.upload(&png(2));
        let refused = rig.call(method, json!({}));
        assert_eq!(kind(&refused), ErrorKind::InvalidParams, "{method}");
        assert!(rig.call("avatar.list", json!({})).is_ok(), "after {method}");
    }
    // A role that may not call it gets the same treatment.
    let agent = rig.connect(Role::Agent, None);
    let id = Uuid::new_v4().to_string();
    let header = attachment_header(&id).unwrap();
    rig.core
        .clients
        .get_mut(&agent)
        .unwrap()
        .attachments
        .push(header, b"bytes")
        .unwrap();
    let forbidden = rig.call_as(&agent, "avatar.list", json!({}));
    assert_eq!(kind(&forbidden), ErrorKind::ForbiddenRole);
    assert!(!rig.core.clients[&agent].attachments.is_pending());
}

#[test]
fn the_same_picture_sent_again_with_the_same_key_is_one_avatar() {
    let mut rig = Rig::new();
    let key = Uuid::new_v4().to_string();
    let mut made = vec![];
    for _ in 0..2 {
        let picture = rig.upload(&png(3));
        made.push(
            rig.call(
                "avatar.create",
                json!({"name": "Sabio", "kind": "generated", "picture": picture,
                       "idempotencyKey": key}),
            )
            .unwrap(),
        );
    }
    assert_eq!(made[0], made[1]);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["avatars"].as_array().unwrap().len(), 1);
    // Another picture under the same key is another request.
    let picture = rig.upload(&png(4));
    let other = rig.call(
        "avatar.create",
        json!({"name": "Sabio", "kind": "generated", "picture": picture, "idempotencyKey": key}),
    );
    assert_eq!(kind(&other), ErrorKind::IdempotencyConflict);
}

#[test]
fn the_library_holds_the_most_the_contract_says() {
    let mut rig = Rig::new();
    for index in 0..limits::AVATAR_COUNT {
        rig.create(&format!("Avatar {index}"), &png(1)).unwrap();
    }
    assert_eq!(
        kind(&rig.create("One more", &png(1))),
        ErrorKind::InvalidParams
    );
}

#[test]
fn two_avatars_may_have_the_same_name() {
    let mut rig = Rig::new();
    let first = rig.avatar("Sabio");
    let second = rig.avatar("Sabio");
    assert_ne!(first.avatar_id, second.avatar_id);
    assert_eq!(first.name, second.name);
}

#[test]
fn an_avatar_is_renamed_and_its_clients_are_told() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let renamed = rig
        .call(
            "avatar.rename",
            json!({"avatarId": avatar.avatar_id, "name": "Mentor"}),
        )
        .unwrap();
    assert_eq!(renamed["avatar"]["name"], "Mentor");
    assert_eq!(rig.changes().last().unwrap()["avatar"]["name"], "Mentor");
    let missing = rig.call("avatar.rename", json!({"avatarId": "nobody", "name": "X"}));
    assert_eq!(kind(&missing), ErrorKind::NotFound);
    let blank = rig.call(
        "avatar.rename",
        json!({"avatarId": avatar.avatar_id, "name": " "}),
    );
    assert_eq!(kind(&blank), ErrorKind::InvalidParams);
}

/// Every file the database keeps, joined: the main file, the write-ahead log and the index of it.
fn database_bytes(directory: &Path) -> Vec<u8> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("soyeht.db") {
            all.extend(std::fs::read(entry.path()).unwrap());
        }
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn an_assigned_avatar_binds_the_workspace_and_a_key_in_the_request_is_never_kept() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let bound = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            "0",
            json!({"model": "a-model", "effort": "high", "persona": "persona-marker-text",
                   "hearing": {"providerId": "cloud", "language": "pt-BR", "apiKey": "sk-secret"},
                   "voice": {"providerId": "cloud", "voiceId": "v1", "token": "sk-secret"},
                   "apiKey": "sk-secret"}),
        )
        .unwrap();
    let binding = &bound["binding"];
    assert_eq!(binding["harness"], "claude-code");
    assert_eq!(binding["persona"], "persona-marker-text");
    assert_eq!(binding["revision"], "1");
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0], *binding);
    let events = rig.changes();
    assert_eq!(events.last().unwrap()["workspaceId"], workspace);
    assert_eq!(events.last().unwrap()["binding"], *binding);
    // The keys went nowhere: not in an answer, an event or what the database holds. The persona is
    // found in the same bytes, so the search reads what holds the data.
    assert!(!listed.to_string().contains("sk-secret"));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("sk-secret")
    );
    let stored = database_bytes(&rig.directory);
    assert!(contains(&stored, "persona-marker-text"));
    assert!(!contains(&stored, "sk-secret"));
}

#[test]
fn an_assignment_needs_an_open_workspace_and_an_avatar_of_the_library() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let unknown_workspace = rig.assign("nobody", &avatar.avatar_id, "0", json!({}));
    assert_eq!(kind(&unknown_workspace), ErrorKind::NotFound);
    let unknown_avatar = rig.assign(&workspace, "nobody", "0", json!({}));
    assert_eq!(kind(&unknown_avatar), ErrorKind::NotFound);
    rig.call("workspace.close", json!({"workspaceId": workspace}))
        .unwrap();
    let closed = rig.assign(&workspace, &avatar.avatar_id, "0", json!({}));
    assert_eq!(kind(&closed), ErrorKind::NotFound);
}

#[test]
fn a_client_with_an_old_copy_cannot_overwrite_a_newer_binding() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let phone = rig.connect(Role::HumanDevice, Some("phone-1"));
    rig.assign(
        &workspace,
        &avatar.avatar_id,
        "0",
        json!({"persona": "v1", "model": "m1"}),
    )
    .unwrap();
    // The computer and the phone both read revision 1. The computer changes the persona.
    let newer = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            "1",
            json!({"persona": "v2", "model": "m1"}),
        )
        .unwrap();
    assert_eq!(newer["binding"]["revision"], "2");
    // The phone writes from what it read: refused, and nothing changes.
    let stale = rig.assign_as(
        &phone,
        &workspace,
        &avatar.avatar_id,
        "1",
        json!({"persona": "v1"}),
    );
    assert_eq!(kind(&stale), ErrorKind::StaleVersion);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0]["persona"], "v2");
    assert_eq!(listed["bindings"][0]["model"], "m1");
    assert_eq!(listed["bindings"][0]["revision"], "2");
    // After it reads again, it writes.
    let fresh = rig
        .assign_as(
            &phone,
            &workspace,
            &avatar.avatar_id,
            "2",
            json!({"persona": "v3"}),
        )
        .unwrap();
    assert_eq!(fresh["binding"]["revision"], "3");
    // A first assignment that thinks the workspace already has one is stale too.
    let other = rig.workspace("other");
    let early = rig.assign(&other, &avatar.avatar_id, "1", json!({}));
    assert_eq!(kind(&early), ErrorKind::StaleVersion);
}

#[test]
fn an_avatar_that_a_workspace_has_is_not_deleted_until_the_workspace_changes_or_closes() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let other = rig.avatar("Other");
    let workspace = rig.workspace("project");
    rig.assign(
        &workspace,
        &avatar.avatar_id,
        "0",
        json!({"persona": "secret persona"}),
    )
    .unwrap();
    let busy = rig.call("avatar.delete", json!({"avatarId": avatar.avatar_id}));
    assert_eq!(kind(&busy), ErrorKind::PaneBusy);
    assert!(rig.folder(&avatar.avatar_id).exists());
    rig.assign(&workspace, &other.avatar_id, "1", json!({}))
        .unwrap();
    let gone = rig
        .call("avatar.delete", json!({"avatarId": avatar.avatar_id}))
        .unwrap();
    assert_eq!(gone, json!({"avatarId": avatar.avatar_id, "removed": true}));
    assert!(!rig.folder(&avatar.avatar_id).exists());
    let last = rig.changes().pop().unwrap();
    assert_eq!(last["avatarId"], avatar.avatar_id);
    assert!(last.get("avatar").is_none());
    // Closing the workspace takes its binding, and the persona in it, with it.
    rig.call("workspace.close", json!({"workspaceId": workspace}))
        .unwrap();
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"], json!([]));
    rig.call("avatar.delete", json!({"avatarId": other.avatar_id}))
        .unwrap();
    let again = rig.call("avatar.delete", json!({"avatarId": other.avatar_id}));
    assert_eq!(kind(&again), ErrorKind::NotFound);
}

#[test]
fn an_agent_and_a_chief_have_none_of_the_avatar_methods() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let picture = json!({"attachmentId": Uuid::new_v4().to_string(), "mimeType": "image/png",
                         "name": "face.png", "bytes": "64"});
    let calls = [
        ("avatar.list", json!({})),
        (
            "avatar.create",
            json!({"name": "X", "kind": "uploaded", "picture": picture}),
        ),
        (
            "avatar.rename",
            json!({"avatarId": avatar.avatar_id, "name": "Y"}),
        ),
        ("avatar.delete", json!({"avatarId": avatar.avatar_id})),
        (
            "avatar.assign",
            json!({"workspaceId": workspace, "avatarId": avatar.avatar_id,
                   "harness": "claude-code", "baseRevision": "0"}),
        ),
    ];
    assert_eq!(calls.len(), 5);
    for role in [Role::Agent, Role::Chief] {
        let connection = rig.connect(role, None);
        for (method, params) in &calls {
            let refused = rig.call_as(&connection, method, params.clone());
            assert_eq!(kind(&refused), ErrorKind::ForbiddenRole, "{method}");
        }
    }
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["avatars"].as_array().unwrap().len(), 1);
    assert_eq!(listed["bindings"], json!([]));
}

#[test]
fn the_library_is_still_there_when_the_engine_starts_again_and_orphan_folders_are_swept() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    rig.assign(
        &workspace,
        &avatar.avatar_id,
        "0",
        json!({"persona": "Be brief."}),
    )
    .unwrap();
    let before = rig.call("avatar.list", json!({})).unwrap();
    // What a crash leaves: the folder of an avatar whose creation never committed, and a stray file.
    let orphan = rig
        .directory
        .join(AVATARS_DIRECTORY)
        .join("never-committed");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join(SOURCE_FILE), png(9)).unwrap();
    std::fs::write(
        rig.directory.join(AVATARS_DIRECTORY).join("stray.txt"),
        b"x",
    )
    .unwrap();
    rig.restart();
    assert_eq!(rig.call("avatar.list", json!({})).unwrap(), before);
    assert!(!orphan.exists());
    assert!(
        !rig.directory
            .join(AVATARS_DIRECTORY)
            .join("stray.txt")
            .exists()
    );
    assert!(rig.folder(&avatar.avatar_id).join(SOURCE_FILE).exists());
}

#[test]
fn the_engine_offers_the_avatars_capability() {
    let rig = Rig::new();
    assert!(rig.core.capabilities().contains(&"avatars".to_owned()));
}
