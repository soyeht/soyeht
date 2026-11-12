//! Path traversal through every method that takes a path. A workspace holds a project folder;
//! a second folder beside it holds a secret and is reached by `..`, by an absolute path, by a
//! link inside the project and by the spellings of other systems. Each attempt is refused with
//! an exact error code, returns nothing of the secret, and changes nothing on disk.
use super::native_permissions_and_modes::run_turn;
use super::*;
use std::collections::BTreeMap;

const SECRET: &str = "outside-secret-bytes";

fn kind(answer: &Value) -> &str {
    answer["error"]["data"]["kind"]
        .as_str()
        .unwrap_or("<no error>")
}

/// A project folder with one file, and a folder beside it that the project must never reach.
struct Site {
    project: PathBuf,
    outside: PathBuf,
}

impl Site {
    fn new(rig: &Rig) -> Self {
        let base = soyeht_platform::fs::canonicalize(&rig.path).unwrap();
        let project = base.join("project");
        let outside = base.join("outside");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(outside.join("inner")).unwrap();
        fs::write(project.join("ok.txt"), "inside").unwrap();
        fs::write(project.join("src/lib.rs"), "fn main() {}\n").unwrap();
        fs::write(outside.join("secret.txt"), SECRET).unwrap();
        fs::write(outside.join("secret.md"), SECRET).unwrap();
        fs::write(outside.join("inner/deep.txt"), SECRET).unwrap();
        symlink(&outside, project.join("out_link")).unwrap();
        symlink(outside.join("secret.txt"), project.join("file_link")).unwrap();
        symlink("../outside", project.join("relative_link")).unwrap();
        Self { project, outside }
    }

    /// Every file under the folder beside the project, with its bytes.
    fn outside_state(&self) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut state = BTreeMap::new();
        let mut pending = vec![self.outside.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    state.insert(path.clone(), None);
                    pending.push(path);
                } else {
                    state.insert(path.clone(), fs::read(&path).ok());
                }
            }
        }
        state
    }

    /// The ways to name something outside the project that every system reads as one.
    fn escapes(&self) -> Vec<String> {
        let outside = self.outside.display();
        let mut found: Vec<String> = [
            "../outside/secret.txt",
            "src/../../outside/secret.txt",
            "..",
            "../",
            ".",
            "../project/../outside/secret.txt",
            "/etc/passwd",
            "//server/share/secret.txt",
            "out_link/secret.txt",
            "out_link/inner/deep.txt",
            "out_link/new.txt",
            "relative_link/secret.txt",
            "./out_link/../secret.txt",
            "src/../out_link/secret.txt",
        ]
        .map(String::from)
        .to_vec();
        found.push(format!("{outside}/secret.txt"));
        found
    }

    /// The spellings of other systems. On this system they are names of files like any other;
    /// on Windows they are drives, shares and devices, and leave the project.
    fn foreign_spellings() -> [&'static str; 6] {
        [
            "C:\\Windows\\win.ini",
            "C:win.ini",
            "\\\\server\\share\\secret.txt",
            "\\\\?\\C:\\Windows\\win.ini",
            "\\\\.\\COM1",
            "..\\outside\\secret.txt",
        ]
    }
}

/// The refusal of a path that leaves the project: `invalid_params`, with no part of the secret.
fn assert_escape_refused(answer: &Value, what: &str) {
    assert_eq!(kind(answer), "invalid_params", "{what}: {answer}");
    assert!(
        !answer.to_string().contains(SECRET),
        "{what} leaked the secret: {answer}"
    );
}

/// A name that does not exist is refused with a code of its own, but never opened.
fn assert_not_opened(answer: &Value, what: &str) {
    assert!(
        matches!(
            kind(answer),
            "invalid_params" | "not_found" | "internal_error"
        ),
        "{what}: {answer}"
    );
    assert!(
        !answer.to_string().contains(SECRET),
        "{what} leaked the secret: {answer}"
    );
}

fn setup() -> (Rig, Site, Client, String) {
    let rig = Rig::new();
    let site = Site::new(&rig);
    let mut client = rig.client();
    let workspace = client.add_workspace(&site.project, "attack-workspace");
    (rig, site, client, workspace)
}

#[test]
fn file_open_refuses_every_path_that_leaves_the_project() {
    let (_rig, site, mut client, workspace) = setup();
    let before = site.outside_state();
    let open = |client: &mut Client, path: &str, index: usize| {
        client.request(
            "file.open",
            json!({"workspaceId":workspace,"path":path,"preview":true,
                "idempotencyKey":format!("open-{index}")}),
        )
    };
    for (index, path) in site.escapes().iter().enumerate() {
        let answer = open(&mut client, path, index);
        assert_not_opened(&answer, &format!("file.open {path:?}"));
        assert!(
            answer.get("result").is_none(),
            "file.open {path:?} opened a file"
        );
    }
    // The links themselves point out of the project, so opening one is opening the target.
    for (index, path) in ["out_link", "file_link"].iter().enumerate() {
        let answer = open(&mut client, path, 100 + index);
        assert_escape_refused(&answer, &format!("file.open {path:?}"));
    }
    // What is inside the project opens, by its relative or its absolute name.
    assert!(client.open_file(&workspace, "ok.txt")["file"]["fileId"].is_string());
    let absolute = site.project.join("src/lib.rs").display().to_string();
    let opened = open(&mut client, &absolute, 200);
    assert_eq!(opened["result"]["file"]["path"], "src/lib.rs", "{opened}");
    assert_eq!(site.outside_state(), before);
}

#[test]
fn the_project_tree_lists_and_watches_only_the_project() {
    let (_rig, site, mut client, workspace) = setup();
    for path in site
        .escapes()
        .iter()
        .map(String::as_str)
        .chain(["out_link", "file_link"])
    {
        let listed = client.request(
            "project.tree.list",
            json!({"workspaceId":workspace,"path":path}),
        );
        assert_escape_refused(&listed, &format!("project.tree.list {path:?}"));
        let watched = client.request(
            "project.tree.watch",
            json!({"workspaceId":workspace,"paths":[path]}),
        );
        assert_escape_refused(&watched, &format!("project.tree.watch {path:?}"));
    }
    let listed = client.call(
        "project.tree.list",
        json!({"workspaceId":workspace,"path":""}),
    );
    assert!(!listed.to_string().contains("secret"), "{listed}");
}

#[test]
fn creating_and_deleting_entries_never_reach_outside_the_project() {
    let (_rig, site, mut client, workspace) = setup();
    let before = site.outside_state();
    for (index, path) in site.escapes().iter().enumerate() {
        for entry_kind in ["file", "directory"] {
            let created = client.request(
                "project.entry.create",
                json!({"workspaceId":workspace,"path":path,"kind":entry_kind,
                    "idempotencyKey":format!("create-{index}-{entry_kind}")}),
            );
            assert_escape_refused(&created, &format!("project.entry.create {path:?}"));
        }
        for permanent in [false, true] {
            let deleted = client.request(
                "project.entry.delete",
                json!({"workspaceId":workspace,"path":path,"permanent":permanent,
                    "idempotencyKey":format!("delete-{index}-{permanent}")}),
            );
            assert_escape_refused(&deleted, &format!("project.entry.delete {path:?}"));
        }
    }
    assert_eq!(site.outside_state(), before);
    assert!(site.outside.join("secret.txt").exists());
    assert!(!site.outside.join("new.txt").exists());
}

/// Creates a file named `name` in the project.
fn create_file(client: &mut Client, workspace: &str, name: &str, index: usize) -> Value {
    client.request(
        "project.entry.create",
        json!({"workspaceId":workspace,"path":name,"kind":"file",
            "idempotencyKey":format!("foreign-{index}")}),
    )
}

/// Names that leave the project on Windows (drives, shares, devices) are file names like any
/// other elsewhere: here they create a file inside the project under that name; on Windows
/// they are refused like any other path outside the project. Nothing outside changes either way.
#[test]
fn the_spellings_of_other_systems_are_contained_by_the_project() {
    let (_rig, site, mut client, workspace) = setup();
    let before = site.outside_state();
    for (index, name) in Site::foreign_spellings().iter().enumerate() {
        let created = create_file(&mut client, &workspace, name, index);
        if cfg!(windows) {
            assert_escape_refused(&created, &format!("project.entry.create {name:?}"));
        } else {
            assert!(created.get("error").is_none(), "{name:?}: {created}");
            assert!(
                site.project.join(name).is_file(),
                "{name:?} is not in the project"
            );
        }
        let opened = client.request(
            "file.open",
            json!({"workspaceId":workspace,"path":name,"preview":true,
                "idempotencyKey":format!("foreign-open-{index}")}),
        );
        assert!(!opened.to_string().contains(SECRET), "{name:?}: {opened}");
    }
    assert_eq!(site.outside_state(), before);
}

/// Every file under the project folder, so a creation can be found wherever it landed.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap().flatten() {
            let path = entry.path();
            let is_link = fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink();
            if path.is_dir() && !is_link {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn writing_documents_are_created_and_opened_only_inside_their_section_of_the_project() {
    let (_rig, site, mut client, workspace) = setup();
    let before = site.outside_state();
    for (index, folder) in ["../outside", "/tmp", "..", "../../.."].iter().enumerate() {
        let created = client.request(
            "writing.document.create",
            json!({"workspaceId":workspace,"section":"texts","title":"Attack",
                "relativeFolder":folder,"idempotencyKey":format!("doc-{index}")}),
        );
        assert_eq!(
            kind(&created),
            "writing_section_refused",
            "{folder:?}: {created}"
        );
    }
    // A folder named like a link is a new folder of the section, never the link's target.
    let files_before = files_under(&site.project).len();
    for (index, folder) in ["out_link", "relative_link/inner"].iter().enumerate() {
        let created = client.request(
            "writing.document.create",
            json!({"workspaceId":workspace,"section":"texts","title":"Inside",
                "relativeFolder":folder,"idempotencyKey":format!("doc-link-{index}")}),
        );
        if created.get("error").is_none() {
            assert!(
                files_under(&site.project).len() > files_before,
                "{folder:?} reported success and wrote nothing in the project"
            );
        }
    }
    for (index, path) in ["../outside/secret.md", "out_link/secret.md", "/etc/passwd"]
        .iter()
        .enumerate()
    {
        let opened = client.request(
            "writing.document.open",
            json!({"workspaceId":workspace,"path":path,"kind":"text",
                "idempotencyKey":format!("doc-open-{index}")}),
        );
        assert_eq!(
            kind(&opened),
            "writing_section_refused",
            "{path:?}: {opened}"
        );
        assert!(!opened.to_string().contains(SECRET));
    }
    assert_eq!(site.outside_state(), before);
}

#[test]
fn a_workspace_cannot_be_made_on_a_path_that_does_not_exist_or_is_not_a_folder() {
    let (rig, site, mut client, _) = setup();
    for (index, path) in [
        site.project.join("ok.txt"),
        site.project.join("missing"),
        rig.path.join("nope/../../nope"),
    ]
    .iter()
    .enumerate()
    {
        let answer = client.request(
            "workspace.create",
            json!({"name":"x","path":path,"idempotencyKey":format!("ws-{index}")}),
        );
        assert!(answer.get("error").is_some(), "{path:?}: {answer}");
        assert!(!answer.to_string().contains(SECRET));
    }
}

/// What the engine answered the agent's last file request in `pane`: the text of its last
/// reply (the fake agent reports the answer it got as the reply to the prompt).
fn agent_file_request(client: &mut Client, pane: &str, method: &str, path: &str) -> String {
    let text = format!("{method}:{path}");
    let before = replies(client, pane).len();
    run_turn(client, pane, &text);
    wait_until("the agent's reply", || replies(client, pane).len() > before);
    replies(client, pane).pop().unwrap_or_default()
}

fn replies(client: &mut Client, pane: &str) -> Vec<String> {
    transcript_items(client, pane)
        .iter()
        .filter(|item| item["kind"] == "message" && item["role"] == "assistant")
        .filter_map(|item| item["text"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn an_agent_cannot_read_or_write_a_file_outside_its_worktree() {
    let rig = Rig::new();
    let site = Site::new(&rig);
    let mut client = rig.client();
    let workspace = client.add_workspace(&site.project, "agent-workspace");
    let (mut client, pane) = rig.open_agent_announcing(client, &json!(workspace), "config");
    let before = site.outside_state();
    let outside = site.outside.display().to_string();
    let project = site.project.display().to_string();
    // The control: the same two requests inside the project succeed, so the refusals below
    // are the engine's rule about the path and not a fake agent or a rig that refuses everything.
    let reply = agent_file_request(
        &mut client,
        &pane,
        "fs-write",
        &format!("{project}/new.txt"),
    );
    assert!(
        !reply.contains("\"error\""),
        "a write inside the project: {reply}"
    );
    assert_eq!(
        fs::read_to_string(site.project.join("new.txt")).unwrap(),
        "attacker\n"
    );
    let reply = agent_file_request(&mut client, &pane, "fs-read", &format!("{project}/ok.txt"));
    assert!(
        !reply.contains("\"error\"") && reply.contains("inside"),
        "a read inside the project: {reply}"
    );
    let attempts = [
        format!("{outside}/secret.txt"),
        format!("{outside}/created-by-agent.txt"),
        format!("{project}/../outside/secret.txt"),
        format!("{project}/out_link/secret.txt"),
        format!("{project}/out_link/created-by-agent.txt"),
        format!("{project}/file_link"),
        format!("{project}/relative_link/created-by-agent.txt"),
        "/etc/passwd".to_owned(),
    ];
    for path in &attempts {
        for method in ["fs-write", "fs-read"] {
            let reply = agent_file_request(&mut client, &pane, method, path);
            assert!(
                reply.contains("\"error\""),
                "{method} {path:?} was not refused: {reply}"
            );
            assert!(!reply.contains(SECRET), "{method} {path:?} leaked: {reply}");
        }
    }
    // A relative path is not a path the engine will resolve for an agent.
    let reply = agent_file_request(&mut client, &pane, "fs-write", "relative.txt");
    assert!(
        reply.contains("Agent file path must be absolute"),
        "{reply}"
    );
    assert!(!site.project.join("relative.txt").exists());
    assert_eq!(site.outside_state(), before);
    assert_eq!(
        fs::read_to_string(site.outside.join("secret.txt")).unwrap(),
        SECRET
    );
}

#[cfg(unix)]
#[test]
fn a_pipe_a_link_loop_or_a_huge_file_in_the_project_is_refused_without_hanging() {
    let (_rig, site, mut client, workspace) = setup();
    let pipe = site.project.join("pipe");
    let made = Command::new("mkfifo").arg(&pipe).status().unwrap();
    assert!(made.success());
    symlink(site.project.join("loop_b"), site.project.join("loop_a")).unwrap();
    symlink(site.project.join("loop_a"), site.project.join("loop_b")).unwrap();
    let huge = fs::File::create(site.project.join("huge.txt")).unwrap();
    huge.set_len(soyeht_protocol::limits::EDITOR_HISTORY_BYTES as u64 + 1)
        .unwrap();
    let started = Instant::now();
    for (path, expected) in [
        ("pipe", "invalid_params"),
        ("loop_a", "internal_error"),
        ("huge.txt", "file_too_large"),
        ("src", "invalid_params"),
    ] {
        let answer = client.request(
            "file.open",
            json!({"workspaceId":workspace,"path":path,"preview":true,
                "idempotencyKey":format!("special-{path}")}),
        );
        assert_eq!(kind(&answer), expected, "{path}: {answer}");
    }
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "a special file held the engine"
    );
    assert!(client.call("engine.status", json!({})).is_object());
}
