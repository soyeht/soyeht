//! The avatar library through the engine's own dispatch (ADR 0166, contract 19).

use super::*;
use crate::{outbox, test_panes};
use soyeht_protocol::{Actor, ErrorKind, Role, attachment::attachment_header};
use std::sync::Arc;

type Answer = std::result::Result<Outcome, errors::Error>;

struct Rig {
    core: Core,
    directory: PathBuf,
    local: String,
}

/// The first bytes of a PNG file and a few more: what the library checks, and no image.
fn png(extra: u8) -> Vec<u8> {
    let mut bytes = PNG_SIGNATURE.to_vec();
    bytes.extend_from_slice(&[extra; 24]);
    bytes
}

impl Rig {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("soyeht-avatars-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        core.link_supervisor(Box::new(test_panes::Answering(|_| {
            Ok(soyeht_ptyd::wire::Response::Ok)
        })));
        let mut rig = Self {
            core,
            directory,
            local: String::new(),
        };
        rig.local = rig.connect(Role::HumanLocal, None);
        rig
    }

    /// A connection that has said `hello`: the person at the computer, or a paired device.
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
        self.core
            .clients
            .get_mut(&local)
            .unwrap()
            .attachments
            .push(header, bytes)
            .unwrap();
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

    fn assign(&mut self, workspace: &str, avatar: &str, extra: Value) -> Result<Value> {
        let mut params =
            json!({"workspaceId": workspace, "avatarId": avatar, "harness": "claude-code"});
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        self.call("avatar.assign", params)
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

    /// Puts an avatar in a state no call of this slice reaches: the builder's (slice 9).
    fn force_state(&self, avatar: &str, state: &str, failure: Option<&str>) {
        let hash = "b".repeat(64);
        soyeht_store::test_support::DatabaseProbe::open(&self.directory)
            .unwrap()
            .set_avatar_build(
                avatar,
                state,
                failure,
                (state == "ready").then_some(hash.as_str()),
            )
            .unwrap();
    }

    fn package(&self, avatar: &str) -> PathBuf {
        avatar_directory(&self.core, avatar).join(PACKAGE_DIRECTORY)
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
fn an_avatar_is_made_from_a_picture_and_is_listed_with_nothing_built() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("  Sabio ");
    assert_eq!(avatar.name, "Sabio");
    assert_eq!(avatar.build_state, AvatarBuildState::None);
    assert_eq!(avatar.source_hash, hex_digest(&png(1)));
    let source = avatar_directory(&rig.core, &avatar.avatar_id).join(SOURCE_FILE);
    assert_eq!(std::fs::read(source).unwrap(), png(1));
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["avatars"][0]["avatarId"], avatar.avatar_id);
    assert_eq!(listed["bindings"], json!([]));
    let events = rig.changes();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["avatar"]["name"], "Sabio");
    assert!(events[0].get("binding").is_none());
}

#[test]
fn a_picture_that_is_not_a_png_or_a_name_with_nothing_in_it_makes_nothing() {
    let mut rig = Rig::new();
    let refused = rig.create("Sabio", b"GIF89a not a png");
    assert_eq!(kind(&refused), ErrorKind::InvalidParams);
    assert_eq!(kind(&rig.create("   ", &png(1))), ErrorKind::InvalidParams);
    let mut empty = rig.upload(&png(1));
    empty["mimeType"] = json!("image/jpeg");
    let jpeg = rig.call(
        "avatar.create",
        json!({"name": "Sabio", "kind": "uploaded", "picture": empty}),
    );
    assert_eq!(kind(&jpeg), ErrorKind::InvalidParams);
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["avatars"], json!([]));
    assert!(rig.changes().is_empty());
    assert!(
        !rig.directory.join(AVATARS_DIRECTORY).exists()
            || std::fs::read_dir(rig.directory.join(AVATARS_DIRECTORY))
                .unwrap()
                .next()
                .is_none()
    );
    // What was sent and refused is not left for the next request.
    assert!(rig.call("avatar.list", json!({})).is_ok());
}

#[test]
fn a_picture_over_the_limit_is_refused() {
    let mut rig = Rig::new();
    let mut big = PNG_SIGNATURE.to_vec();
    big.resize(limits::AVATAR_PICTURE_BYTES + 1, 0);
    // The connection takes at most the largest attachment the contract names, so the transfer may
    // fail on its own: either way, nothing is made.
    let id = Uuid::new_v4().to_string();
    let local = rig.local.clone();
    let header = attachment_header(&id).unwrap();
    let client = rig.core.clients.get_mut(&local).unwrap();
    for chunk in big.chunks(limits::ATTACHMENT_PAYLOAD_BYTES) {
        client.attachments.push(header, chunk).unwrap();
    }
    let picture = json!({"attachmentId": id, "mimeType": "image/png", "name": "big.png",
                         "bytes": big.len().to_string()});
    let made = rig.call(
        "avatar.create",
        json!({"name": "Big", "kind": "uploaded", "picture": picture}),
    );
    assert!(made.is_err());
    assert_eq!(
        rig.call("avatar.list", json!({})).unwrap()["avatars"],
        json!([])
    );
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
                json!({"name": "Sabio", "kind": "generated", "picture": picture, "idempotencyKey": key}),
            )
            .unwrap(),
        );
    }
    assert_eq!(made[0], made[1]);
    assert_eq!(
        rig.call("avatar.list", json!({})).unwrap()["avatars"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
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
        let connection = rig.local.clone();
        let picture = rig.upload(&png(1));
        rig.call_as(
            &connection,
            "avatar.create",
            json!({"name": format!("Avatar {index}"), "kind": "uploaded", "picture": picture}),
        )
        .unwrap();
    }
    assert_eq!(
        kind(&rig.create("One more", &png(1))),
        ErrorKind::InvalidParams
    );
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

#[test]
fn an_assigned_avatar_binds_the_workspace_and_a_key_in_the_request_is_never_kept() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    let bound = rig
        .assign(
            &workspace,
            &avatar.avatar_id,
            json!({"model": "a-model", "effort": "high", "persona": "Speak briefly.",
                   "hearing": {"providerId": "cloud", "language": "pt-BR", "apiKey": "sk-secret"},
                   "voice": {"providerId": "cloud", "voiceId": "v1", "token": "sk-secret"},
                   "apiKey": "sk-secret", "voiceOn": true}),
        )
        .unwrap();
    let binding = &bound["binding"];
    assert_eq!(binding["harness"], "claude-code");
    assert_eq!(binding["persona"], "Speak briefly.");
    assert_eq!(binding["voiceOn"], json!(["local"]));
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"][0], *binding);
    // The provider keys went nowhere: not in the answer, not in an event, not in the database.
    let events = rig.changes();
    assert_eq!(events.last().unwrap()["workspaceId"], workspace);
    assert_eq!(events.last().unwrap()["binding"], *binding);
    assert!(!listed.to_string().contains("sk-secret"));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("sk-secret")
    );
    let everything = std::fs::read(rig.directory.join("soyeht.db")).unwrap();
    assert!(!String::from_utf8_lossy(&everything).contains("sk-secret"));
}

#[test]
fn an_assignment_needs_an_open_workspace_and_an_avatar_of_the_library() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    assert_eq!(
        kind(&rig.assign("nobody", &avatar.avatar_id, json!({}))),
        ErrorKind::NotFound
    );
    assert_eq!(
        kind(&rig.assign(&workspace, "nobody", json!({}))),
        ErrorKind::NotFound
    );
    rig.call("workspace.close", json!({"workspaceId": workspace}))
        .unwrap();
    assert_eq!(
        kind(&rig.assign(&workspace, &avatar.avatar_id, json!({}))),
        ErrorKind::NotFound
    );
}

/// A library of two avatars and one open workspace.
fn rig_with_two_avatars_and_a_workspace() -> (Rig, Avatar, Avatar, String) {
    let mut rig = Rig::new();
    let first = rig.avatar("Sabio");
    let second = rig.avatar("Other");
    let workspace = rig.workspace("project");
    (rig, first, second, workspace)
}

#[test]
fn voice_is_on_per_client_and_a_new_assignment_does_not_touch_the_others() {
    let (mut rig, avatar, other, workspace) = rig_with_two_avatars_and_a_workspace();
    let phone = rig.connect(Role::HumanDevice, Some("phone-1"));
    rig.assign(&workspace, &avatar.avatar_id, json!({"voiceOn": true}))
        .unwrap();
    let from_phone = rig.call_as(
        &phone,
        "avatar.assign",
        json!({"workspaceId": workspace, "avatarId": avatar.avatar_id, "harness": "codex",
               "voiceOn": true}),
    );
    assert_eq!(
        from_phone.unwrap()["binding"]["voiceOn"],
        json!(["local", "phone-1"])
    );
    // Rebinding without `voiceOn` leaves every client as it was.
    let rebound = rig.assign(&workspace, &other.avatar_id, json!({})).unwrap();
    assert_eq!(rebound["binding"]["voiceOn"], json!(["local", "phone-1"]));
    assert_eq!(rebound["binding"]["avatarId"], other.avatar_id);
    // Only the caller's own entry follows `voiceOn: false`.
    let off = rig
        .assign(&workspace, &other.avatar_id, json!({"voiceOn": false}))
        .unwrap();
    assert_eq!(off["binding"]["voiceOn"], json!(["phone-1"]));
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["bindings"].as_array().unwrap().len(), 1);
}

#[test]
fn an_avatar_that_a_workspace_uses_is_not_deleted_until_the_workspace_is_gone_or_changes() {
    let (mut rig, avatar, other, workspace) = rig_with_two_avatars_and_a_workspace();
    rig.assign(&workspace, &avatar.avatar_id, json!({}))
        .unwrap();
    let busy = rig.call("avatar.delete", json!({"avatarId": avatar.avatar_id}));
    assert_eq!(kind(&busy), ErrorKind::PaneBusy);
    rig.assign(&workspace, &other.avatar_id, json!({})).unwrap();
    let folder = avatar_directory(&rig.core, &avatar.avatar_id);
    assert!(folder.exists());
    let gone = rig
        .call("avatar.delete", json!({"avatarId": avatar.avatar_id}))
        .unwrap();
    assert_eq!(gone, json!({"avatarId": avatar.avatar_id, "removed": true}));
    assert!(!folder.exists());
    let last = rig.changes().pop().unwrap();
    assert_eq!(last["avatarId"], avatar.avatar_id);
    assert!(last.get("avatar").is_none());
    // A closed workspace no longer holds its avatar.
    rig.call("workspace.close", json!({"workspaceId": workspace}))
        .unwrap();
    rig.call("avatar.delete", json!({"avatarId": other.avatar_id}))
        .unwrap();
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(
        (listed["avatars"].clone(), listed["bindings"].clone()),
        (json!([]), json!([]))
    );
    let again = rig.call("avatar.delete", json!({"avatarId": other.avatar_id}));
    assert_eq!(kind(&again), ErrorKind::NotFound);
}

#[test]
fn a_build_is_asked_for_once_and_waits_for_the_builder() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let started = rig
        .call("avatar.build", json!({"avatarId": avatar.avatar_id}))
        .unwrap();
    assert_eq!(started["avatar"]["buildState"], "building");
    let events = rig.changes().len();
    // Asking again while it builds answers what is, and says nothing new.
    let again = rig
        .call("avatar.build", json!({"avatarId": avatar.avatar_id}))
        .unwrap();
    assert_eq!(again["avatar"]["buildState"], "building");
    assert_eq!(rig.changes().len(), events);
    // A failed build is asked for again, and forgets its failure.
    rig.force_state(&avatar.avatar_id, "failed", Some("frames"));
    let listed = rig.call("avatar.list", json!({})).unwrap();
    assert_eq!(listed["avatars"][0]["failureKind"], "frames");
    let retried = rig
        .call("avatar.build", json!({"avatarId": avatar.avatar_id}))
        .unwrap();
    assert_eq!(retried["avatar"]["buildState"], "building");
    assert!(retried["avatar"].get("failureKind").is_none());
    // A built avatar is finished.
    rig.force_state(&avatar.avatar_id, "ready", None);
    let refused = rig.call("avatar.build", json!({"avatarId": avatar.avatar_id}));
    assert_eq!(kind(&refused), ErrorKind::TransitionRefused);
    let missing = rig.call("avatar.build", json!({"avatarId": "nobody"}));
    assert_eq!(kind(&missing), ErrorKind::NotFound);
}

/// A ready avatar whose package holds `manifest.json` and a file in a folder.
fn ready_avatar(rig: &mut Rig) -> Avatar {
    let avatar = rig.avatar("Sabio");
    rig.force_state(&avatar.avatar_id, "ready", None);
    let package = rig.package(&avatar.avatar_id);
    std::fs::create_dir_all(package.join("atlas")).unwrap();
    std::fs::write(package.join("manifest.json"), b"{\"format\":1}").unwrap();
    std::fs::write(
        package.join("atlas/mouth.png"),
        (0..=255u8).collect::<Vec<_>>(),
    )
    .unwrap();
    avatar
}

fn read_package(
    rig: &mut Rig,
    avatar: &str,
    file: &str,
    offset: &str,
    length: Option<u32>,
) -> Answer {
    let mut params = json!({"avatarId": avatar, "file": file, "offset": offset});
    if let Some(length) = length {
        params["length"] = json!(length);
    }
    let local = rig.local.clone();
    rig.outcome(&local, "avatar.package.read", params)
}

fn chunk(answer: Answer) -> (Value, Vec<u8>) {
    match answer.unwrap() {
        Outcome::Attachment {
            result,
            reference,
            bytes,
        } => {
            soyeht_protocol::validate::result("avatar.package.read", &result).unwrap();
            assert_eq!(reference.bytes, bytes.len().to_string());
            assert_eq!(result["chunk"]["attachmentId"], reference.attachment_id);
            (result, bytes)
        }
        _ => panic!("the package read did not answer with an attachment"),
    }
}

#[test]
fn a_ready_package_is_read_in_ranges_that_add_up_to_the_file() {
    let mut rig = Rig::new();
    let avatar = ready_avatar(&mut rig);
    let id = avatar.avatar_id;
    let (whole, bytes) = chunk(read_package(&mut rig, &id, "manifest.json", "0", None));
    assert_eq!(bytes, b"{\"format\":1}");
    assert_eq!(
        (whole["size"].as_str(), whole["offset"].as_str()),
        (Some("12"), Some("0"))
    );
    assert_eq!(whole["chunk"]["name"], "manifest.json");
    let mut joined = vec![];
    for offset in (0..256).step_by(100) {
        let (result, bytes) = chunk(read_package(
            &mut rig,
            &id,
            "atlas/mouth.png",
            &offset.to_string(),
            Some(100),
        ));
        assert_eq!(result["size"], "256");
        joined.extend(bytes);
    }
    assert_eq!(joined, (0..=255u8).collect::<Vec<_>>());
    // At the end there is nothing more, and past it is a mistake.
    let (_, empty) = chunk(read_package(&mut rig, &id, "atlas/mouth.png", "256", None));
    assert!(empty.is_empty());
    let past = read_package(&mut rig, &id, "atlas/mouth.png", "257", None);
    assert_eq!(errors::kind(&past.err().unwrap()), ErrorKind::InvalidParams);
}

#[test]
fn a_package_read_never_leaves_the_package_folder() {
    let mut rig = Rig::new();
    let avatar = ready_avatar(&mut rig);
    let id = avatar.avatar_id;
    std::fs::write(avatar_directory(&rig.core, &id).join("secret.txt"), b"nope").unwrap();
    for file in [
        "../secret.txt",
        "atlas/../../secret.txt",
        "/etc/passwd",
        "atlas//mouth.png",
        "./manifest.json",
        "atlas\\mouth.png",
        "C:manifest.json",
        "atlas/",
    ] {
        let refused = read_package(&mut rig, &id, file, "0", None);
        assert_eq!(
            errors::kind(&refused.err().unwrap()),
            ErrorKind::InvalidParams,
            "{file}"
        );
    }
    let missing = read_package(&mut rig, &id, "nothing.json", "0", None);
    assert_eq!(errors::kind(&missing.err().unwrap()), ErrorKind::NotFound);
    // A folder is not a file.
    let folder = read_package(&mut rig, &id, "atlas", "0", None);
    assert_eq!(errors::kind(&folder.err().unwrap()), ErrorKind::NotFound);
    #[cfg(unix)]
    {
        let package = rig.package(&id);
        std::os::unix::fs::symlink(
            avatar_directory(&rig.core, &id).join("secret.txt"),
            package.join("link.txt"),
        )
        .unwrap();
        let linked = read_package(&mut rig, &id, "link.txt", "0", None);
        assert_eq!(errors::kind(&linked.err().unwrap()), ErrorKind::NotFound);
    }
}

#[test]
fn an_avatar_without_a_ready_package_has_none_to_read() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let package = rig.package(&avatar.avatar_id);
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("manifest.json"), b"{}").unwrap();
    let unready = read_package(&mut rig, &avatar.avatar_id, "manifest.json", "0", None);
    assert_eq!(errors::kind(&unready.err().unwrap()), ErrorKind::NotFound);
    let nobody = read_package(&mut rig, "nobody", "manifest.json", "0", None);
    assert_eq!(errors::kind(&nobody.err().unwrap()), ErrorKind::NotFound);
}

#[test]
fn a_chunk_is_never_larger_than_one_attachment_frame() {
    let mut rig = Rig::new();
    let avatar = ready_avatar(&mut rig);
    let package = rig.package(&avatar.avatar_id);
    std::fs::write(
        package.join("big.bin"),
        vec![9u8; limits::ATTACHMENT_PAYLOAD_BYTES + 10],
    )
    .unwrap();
    let (result, bytes) = chunk(read_package(
        &mut rig,
        &avatar.avatar_id,
        "big.bin",
        "0",
        None,
    ));
    assert_eq!(bytes.len(), limits::ATTACHMENT_PAYLOAD_BYTES);
    assert_eq!(
        result["size"],
        (limits::ATTACHMENT_PAYLOAD_BYTES + 10).to_string()
    );
}

#[test]
fn an_agent_has_no_avatar_methods() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    for role in [Role::Agent, Role::Chief] {
        let agent = rig.connect(role, None);
        let listed = rig.call_as(&agent, "avatar.list", json!({}));
        assert_eq!(kind(&listed), ErrorKind::ForbiddenRole);
        let deleted = rig.call_as(
            &agent,
            "avatar.delete",
            json!({"avatarId": avatar.avatar_id}),
        );
        assert_eq!(kind(&deleted), ErrorKind::ForbiddenRole);
    }
    assert_eq!(
        rig.call("avatar.list", json!({})).unwrap()["avatars"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn the_library_is_still_there_when_the_engine_starts_again() {
    let mut rig = Rig::new();
    let avatar = rig.avatar("Sabio");
    let workspace = rig.workspace("project");
    rig.assign(
        &workspace,
        &avatar.avatar_id,
        json!({"persona": "Be brief.", "voiceOn": true}),
    )
    .unwrap();
    let directory = rig.directory.clone();
    let before = rig.call("avatar.list", json!({})).unwrap();
    let local = rig.local.clone();
    rig.core.disconnect(&local).unwrap();
    rig.core = Core::open_with(&directory, Arc::new(crate::hooks::NoHooks)).unwrap();
    rig.local = rig.connect(Role::HumanLocal, None);
    assert_eq!(rig.call("avatar.list", json!({})).unwrap(), before);
}

#[test]
fn a_package_path_is_judged_before_the_disk_is_touched() {
    let root = std::env::temp_dir().join("soyeht-avatar-no-such-folder");
    for file in ["", "a//b", "a/./b", "../a", "a/..", "/a", "a\\b", "c:a"] {
        assert!(
            matches!(
                package_file(&root, file),
                Err(errors::Error::InvalidParams(_))
            ),
            "{file:?}"
        );
    }
}
