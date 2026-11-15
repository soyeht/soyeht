use super::*;
use crate::{
    core::Core,
    dispatch::Outcome,
    errors,
    outbox::Receiver,
    web::test_support::{connect_local, create_workspace, next_json, reply, request, temp_root},
};
use notify::{Event, EventKind, event::AccessKind};
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, PaneMode, limits};
use std::{fs, sync::mpsc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn resolve(root: &Path, requested: &str) -> Result<ProjectFile> {
    ProjectFile::resolve(root, requested)
}

fn refused(result: Result<ProjectFile>) -> ErrorKind {
    errors::kind(&result.err().expect("the path should be refused"))
}

/// The port of a page's address.
fn port_of(url: &str) -> u16 {
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .and_then(|port| port.parse().ok())
        .expect("a loopback address with a port")
}

/// The path of a page's address.
fn path_of(url: &str) -> String {
    let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
    rest[rest.find('/').unwrap()..].to_owned()
}

/// What a raw HTTP/1.1 request to the page's port answered: the head and the body. `raw` makes the
/// request from the address's path and port.
async fn ask(url: &str, raw: impl Fn(&str, u16) -> String) -> (String, Vec<u8>) {
    let port = port_of(url);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream
        .write_all(raw(&path_of(url), port).as_bytes())
        .await
        .unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.unwrap();
    let split = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("an answer has a head")
        + 4;
    (
        String::from_utf8(answer[..split].to_vec()).unwrap(),
        answer[split..].to_vec(),
    )
}

fn get(path: &str, port: u16) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")
}

fn status(head: &str) -> &str {
    head.lines().next().unwrap()
}

fn started(root: &Path, requested: &str) -> (ProjectPages, ProjectPage) {
    let pages = ProjectPages::default();
    let page = pages
        .start("workspace", resolve(root, requested).unwrap())
        .unwrap();
    (pages, page)
}

/// Whether nothing listens on `port` after the page that did was dropped (its task ends on its
/// next turn).
async fn ends(port: u16) -> bool {
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[test]
fn a_file_of_the_workspace_resolves_to_its_directory() {
    let root = temp_root();
    write(&root, "cards/index.html", "<p>card</p>");
    let file = resolve(&root, "./cards/index.html").unwrap();
    assert_eq!(file.name, "index.html");
    assert!(file.root.ends_with("cards") && file.file.ends_with("cards/index.html"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_path_that_leaves_the_workspace_or_names_no_file_is_refused() {
    let root = temp_root();
    let outer = temp_root();
    write(&outer, "secret.html", "secret");
    write(&root, ".env", "token");
    write(&root, "visible.html", "ok");
    fs::create_dir_all(root.join("folder")).unwrap();
    let outside = outer.join("secret.html").to_string_lossy().into_owned();
    for requested in [
        outside.as_str(),
        "../secret.html",
        "folder/../../secret.html",
        ".env",
        "./.env",
        ".git/config",
    ] {
        assert_eq!(
            refused(resolve(&root, requested)),
            ErrorKind::InvalidParams,
            "{requested}"
        );
    }
    assert_eq!(refused(resolve(&root, "missing.html")), ErrorKind::NotFound);
    assert_eq!(refused(resolve(&root, "folder")), ErrorKind::InvalidParams);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outer.join("secret.html"), root.join("link.html")).unwrap();
        assert_eq!(
            refused(resolve(&root, "link.html")),
            ErrorKind::InvalidParams
        );
        std::os::unix::fs::symlink(root.join(".env"), root.join("shown.html")).unwrap();
        assert_eq!(
            refused(resolve(&root, "shown.html")),
            ErrorKind::InvalidParams
        );
    }
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outer).unwrap();
}

#[tokio::test]
async fn the_page_serves_its_file_and_the_files_below_it_with_the_sandbox_headers() {
    let root = temp_root();
    write(&root, "deck/index.html", "<script src=\"app.js\"></script>");
    write(&root, "deck/app.js", "run()");
    write(&root, "deck/img/a b.png", "png");
    write(&root, "outside.txt", "above the page");
    let (_pages, page) = started(&root, "deck/index.html");
    let entry = page.entry_url().to_owned();
    assert!(entry.starts_with("http://127.0.0.1:") && entry.ends_with("/index.html"));

    let (head, body) = ask(&entry, get).await;
    assert_eq!(status(&head), "HTTP/1.1 200 OK");
    assert_eq!(body, b"<script src=\"app.js\"></script>");
    for header in [
        "Content-Type: text/html; charset=utf-8",
        "Cache-Control: no-store",
        "X-Content-Type-Options: nosniff",
        "Referrer-Policy: no-referrer",
        "Access-Control-Allow-Origin: *",
    ] {
        assert!(head.contains(header), "{header}\n{head}");
    }
    assert!(head.contains(&format!(
        "Content-Security-Policy: {}",
        soyeht_web::CONTENT_SECURITY_POLICY
    )));
    assert!(head.contains(&format!("Content-Length: {}", body.len())));

    let (head, body) = ask(&entry.replace("index.html", "app.js"), get).await;
    assert!(head.contains("Content-Type: text/javascript"));
    assert_eq!(body, b"run()");
    let image = entry.replace("index.html", "img/a%20b.png");
    assert_eq!(ask(&image, get).await.1, b"png");

    let (head, body) = ask(&entry, |path, port| {
        format!("HEAD {path} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n")
    })
    .await;
    assert_eq!(status(&head), "HTTP/1.1 200 OK");
    assert!(body.is_empty() && head.contains("Content-Length: 30"));
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn the_page_answers_nothing_above_its_directory_or_without_its_token() {
    let root = temp_root();
    write(&root, "deck/index.html", "page");
    write(&root, "deck/.hidden.txt", "hidden");
    write(&root, "deck/sub/inner.txt", "inner");
    write(&root, "above.txt", "above the page");
    let outer = temp_root();
    write(&outer, "elsewhere.txt", "elsewhere");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outer.join("elsewhere.txt"), root.join("deck/leak.txt"))
            .unwrap();
        std::os::unix::fs::symlink(root.join("deck/.hidden.txt"), root.join("deck/peek.txt"))
            .unwrap();
    }
    let (_pages, page) = started(&root, "deck/index.html");
    let entry = page.entry_url().to_owned();
    let beside = |tail: &str| entry.replace("index.html", tail);
    for tail in [
        "../above.txt",
        "%2e%2e/above.txt",
        "%2e%2e%2fabove.txt",
        ".hidden.txt",
        "leak.txt",
        "peek.txt",
        "sub",
        "sub/",
        "missing.txt",
    ] {
        let (head, body) = ask(&beside(tail), get).await;
        assert_eq!(status(&head), "HTTP/1.1 404 Not Found", "{tail}");
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("above the page") && !text.contains("elsewhere"));
    }
    assert_eq!(ask(&beside("sub/inner.txt"), get).await.1, b"inner");
    // The same port without the token, and with a raw `..` that no client has normalized.
    for target in ["/", "/index.html", "/0000/index.html"] {
        let (head, _) = ask(&entry, |_, port| get(target, port)).await;
        assert_eq!(status(&head), "HTTP/1.1 404 Not Found", "{target}");
    }
    let climbing = |path: &str, port: u16| {
        let folder = path.rsplit_once('/').unwrap().0;
        get(&format!("{folder}/../../above.txt"), port)
    };
    assert_eq!(
        status(&ask(&entry, climbing).await.0),
        "HTTP/1.1 404 Not Found"
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outer).unwrap();
}

#[tokio::test]
async fn a_request_from_another_host_or_with_another_method_is_refused() {
    let root = temp_root();
    write(&root, "index.html", "page");
    let (_pages, page) = started(&root, "index.html");
    let entry = page.entry_url().to_owned();
    for host in ["evil.example:80", "evil.example", "a.localhost:1"] {
        let (head, body) = ask(&entry, |path, _| {
            format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n")
        })
        .await;
        assert_eq!(status(&head), "HTTP/1.1 403 Forbidden", "{host}");
        assert!(!body.windows(4).any(|word| word == b"page"));
    }
    let (head, _) = ask(&entry, |path, _| format!("GET {path} HTTP/1.1\r\n\r\n")).await;
    assert_eq!(status(&head), "HTTP/1.1 403 Forbidden");
    let (head, _) = ask(&entry, |path, port| {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: evil.example\r\n\r\n")
    })
    .await;
    assert_eq!(status(&head), "HTTP/1.1 400 Bad Request");
    for method in ["POST", "PUT", "DELETE", "OPTIONS"] {
        let (head, _) = ask(&entry, |path, port| {
            format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")
        })
        .await;
        assert_eq!(status(&head), "HTTP/1.1 405 Method Not Allowed", "{method}");
        assert!(head.contains("Allow: GET, HEAD"));
    }
    let (head, _) = ask(&entry, |_, _| "nonsense\r\n\r\n".to_owned()).await;
    assert_eq!(status(&head), "HTTP/1.1 400 Bad Request");
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_file_over_the_limit_is_refused_and_a_page_that_ended_answers_nothing() {
    let root = temp_root();
    write(&root, "index.html", "page");
    let big = fs::File::create(root.join("big.bin")).unwrap();
    big.set_len(u64::try_from(limits::WEB_PROJECT_FILE_BYTES).unwrap() + 1)
        .unwrap();
    let (_pages, page) = started(&root, "index.html");
    let entry = page.entry_url().to_owned();
    let (head, _) = ask(&entry.replace("index.html", "big.bin"), get).await;
    assert_eq!(status(&head), "HTTP/1.1 413 Content Too Large");
    drop(page);
    assert!(ends(port_of(&entry)).await, "the server outlived its page");
    fs::remove_dir_all(root).unwrap();
}

fn event(kind: EventKind, path: &Path) -> notify::Result<Event> {
    Ok(Event::new(kind).add_path(path.to_path_buf()))
}

#[test]
fn a_burst_of_changes_is_one_change_and_a_read_or_a_hidden_file_is_none() {
    let root = temp_root();
    let (sender, receiver) = mpsc::channel();
    let changes = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let kicks = std::sync::atomic::AtomicUsize::new(0);
    let modify = EventKind::Modify(notify::event::ModifyKind::Any);
    std::thread::scope(|scope| {
        let (root, changes, stop, kicks) = (&root, &changes, &stop, &kicks);
        scope.spawn(move || {
            watch::settle(&receiver, root, changes, stop, || {
                kicks.fetch_add(1, Ordering::Relaxed);
            });
        });
        // Serving a file reads it; a hidden file is not the page's.
        sender
            .send(event(
                EventKind::Access(AccessKind::Any),
                &root.join("a.html"),
            ))
            .unwrap();
        sender
            .send(event(modify, &root.join(".git/index")))
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(changes.load(Ordering::Relaxed), 0);
        for _ in 0..5 {
            sender.send(event(modify, &root.join("a.html"))).unwrap();
        }
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(changes.load(Ordering::Relaxed), 1);
        assert_eq!(kicks.load(Ordering::Relaxed), 1);
        // A stream that lost events may hide a change.
        sender
            .send(Err(notify::Error::generic("overflow")))
            .unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(changes.load(Ordering::Relaxed), 2);
        stop.store(true, Ordering::Relaxed);
    });
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn web_open_of_a_project_file_serves_it_in_a_pane_and_ends_with_the_pane() {
    let root = temp_root();
    write(&root, "pages/cards.html", "cards");
    write(&root, "other.html", "other");
    let state = temp_root();
    let mut core = Core::open(&state).unwrap();
    let (connection, _receiver) = connect_local(&mut core, "project-page-test", json!([])).await;
    let workspace = create_workspace(&mut core, &connection, &root);
    let open = |core: &mut Core, id: u64, mut params: Value| {
        params["workspaceId"] = json!(workspace);
        params["idempotencyKey"] = json!(format!("open-{id}"));
        core.dispatch(&connection, request(id, "web.open", params))
    };
    let Ok(Outcome::Reply(first)) = open(&mut core, 3, json!({"path":"pages/cards.html"})) else {
        panic!("opening a project file should reply")
    };
    let pane = first["paneId"].as_str().unwrap().to_owned();
    let url = core.panes[&pane].stored.pane.url.clone().unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:") && url.ends_with("/cards.html"),
        "{url}"
    );
    assert_eq!(ask(&url, get).await.1, b"cards");

    // The same file focuses its pane; a new pane is asked for; another file is another pane.
    let Ok(Outcome::Reply(again)) = open(&mut core, 4, json!({"path":"./pages/cards.html"})) else {
        panic!("opening the same file should reply")
    };
    assert_eq!(again["paneId"], json!(pane));
    let Ok(Outcome::Reply(second)) = open(
        &mut core,
        5,
        json!({"path":"pages/cards.html","newPane":true}),
    ) else {
        panic!("opening a new pane should reply")
    };
    assert_ne!(second["paneId"], json!(pane));
    let Ok(Outcome::Reply(other)) = open(&mut core, 6, json!({"path":"other.html"})) else {
        panic!("opening another file should reply")
    };
    assert_ne!(other["paneId"], json!(pane));
    assert_eq!(
        core.panes
            .values()
            .filter(|pane| pane.stored.pane.mode == PaneMode::Web)
            .count(),
        3
    );

    // The call names exactly one address, inside the workspace, of a file that is there.
    for (id, params) in [
        (7, json!({})),
        (8, json!({"path":"other.html","url":"example.com"})),
        (9, json!({"path":"../escape.html"})),
        (10, json!({"path":"missing.html"})),
    ] {
        assert!(open(&mut core, id, params).is_err(), "{id}");
    }

    reply(
        &mut core,
        &connection,
        request(
            11,
            "web.close",
            json!({"paneId":pane,"idempotencyKey":"close"}),
        ),
    );
    assert!(!core.web.pages.pages.contains_key(&pane));
    assert!(
        ends(port_of(&url)).await,
        "closing the pane left its page served"
    );
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}

async fn nothing_sent(receiver: &mut Receiver) -> bool {
    tokio::time::timeout(Duration::from_millis(150), receiver.next())
        .await
        .is_err()
}

#[tokio::test]
async fn a_changed_project_page_is_reloaded_in_the_pane_that_still_shows_it() {
    let root = temp_root();
    write(&root, "cards.html", "cards");
    let state = temp_root();
    let mut core = Core::open(&state).unwrap();
    let (host, mut host_receiver) =
        connect_local(&mut core, "project-page-host", json!(["web_host"])).await;
    let (caller, _caller_receiver) =
        connect_local(&mut core, "project-page-caller", json!([])).await;
    let workspace = create_workspace(&mut core, &caller, &root);
    let opened = reply(
        &mut core,
        &caller,
        request(
            3,
            "web.open",
            json!({"workspaceId":workspace,"path":"cards.html","idempotencyKey":"open"}),
        ),
    );
    let pane = opened["paneId"].as_str().unwrap().to_owned();
    let changes = Arc::clone(&core.web.pages.pages[&pane].changes);

    core.reload_project_pages().unwrap();
    assert!(
        nothing_sent(&mut host_receiver).await,
        "nothing changed yet"
    );

    changes.fetch_add(1, Ordering::Relaxed);
    core.reload_project_pages().unwrap();
    let command = next_json(&mut host_receiver).await;
    assert_eq!(command["method"], "web.command");
    assert_eq!(command["params"]["paneId"], json!(pane));
    assert_eq!(command["params"]["command"], "navigate");
    assert_eq!(command["params"]["args"]["action"], "reload");
    // The host's answer to a command nobody waits for is dropped without harm.
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":command["id"],"result":{"paneId":pane}}),
    )
    .unwrap();

    // A change that came while the pane showed another address reloads nothing.
    core.panes.get_mut(&pane).unwrap().stored.pane.url = Some("https://example.com/".into());
    changes.fetch_add(1, Ordering::Relaxed);
    core.reload_project_pages().unwrap();
    assert!(nothing_sent(&mut host_receiver).await);
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}
