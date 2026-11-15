//! The server of one project page: HTTP/1.1 over a loopback socket, `GET` and `HEAD` of the files
//! below the page's directory, one request per connection. What a request may name is decided by
//! `soyeht_web::requested_parts`; what is read is decided here, on the canonical path, on every
//! request.

use super::Served;
use crate::auth;
use soyeht_protocol::limits;
use soyeht_web::{
    RangeAnswer, byte_range, content_security_policy, content_type, host_is_page, name_allowed,
    requested_parts,
};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};

/// The largest request head read; a page's requests are a line and a few headers.
const HEAD_BYTES: usize = 8192;
/// How long a client has to send its request, and to take its answer.
const HEAD_PATIENCE: Duration = Duration::from_secs(5);
const WRITE_PATIENCE: Duration = Duration::from_secs(30);

/// What the server of a page answers for.
pub(super) struct Site {
    port: u16,
    token: String,
    /// The page's directory, canonical.
    root: PathBuf,
    /// The page's own document, canonical: asking for it is a load of the page.
    document: PathBuf,
    served: Arc<Served>,
    policy: String,
}

impl Site {
    pub(super) fn new(
        port: u16,
        token: String,
        root: PathBuf,
        document: PathBuf,
        served: Arc<Served>,
    ) -> Self {
        Self {
            policy: content_security_policy(&format!("http://127.0.0.1:{port}")),
            port,
            token,
            root,
            document,
            served,
        }
    }
}

/// Accepts connections until the task is aborted (the pane closed); at most
/// `webProjectPageConnections` are served at once.
pub(super) async fn run(listener: TcpListener, site: Site) {
    let site = Arc::new(site);
    let places = Arc::new(Semaphore::new(limits::WEB_PROJECT_PAGE_CONNECTIONS));
    loop {
        let Ok(place) = Arc::clone(&places).acquire_owned().await else {
            return;
        };
        let Ok((stream, _)) = listener.accept().await else {
            // An accept fails for one connection (a client that went away first); the loop tries
            // the next, and the floor keeps a listener that fails for good from spinning.
            tokio::time::sleep(soyeht_platform::net::ACCEPT_ERROR_FLOOR).await;
            continue;
        };
        let site = Arc::clone(&site);
        tokio::spawn(async move {
            answer(stream, &site).await;
            drop(place);
        });
    }
}

/// One connection: reads the request head, answers it, closes.
async fn answer(mut stream: TcpStream, site: &Site) {
    let Ok(Some(head)) = timeout(HEAD_PATIENCE, read_head(&mut stream)).await else {
        return;
    };
    let reply = reply_to(&head, site).await;
    // ignored: a client that went away or stopped reading is owed nothing more.
    drop(
        timeout(
            WRITE_PATIENCE,
            write_reply(&mut stream, &reply, &site.policy),
        )
        .await,
    );
}

/// The bytes of the request up to the blank line that ends its head; `None` when the client closed
/// first or the head is longer than the limit.
async fn read_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let Ok(read) = stream.read(&mut chunk).await else {
            return None;
        };
        if read == 0 {
            return None;
        }
        head.extend_from_slice(chunk.get(..read)?);
        if head.windows(4).any(|window| window == b"\r\n\r\n") {
            return Some(head);
        }
        if head.len() > HEAD_BYTES {
            return None;
        }
    }
}

struct Request<'a> {
    method: &'a str,
    target: &'a str,
    host: Option<&'a str>,
    range: Option<&'a str>,
}

/// The request line and the `Host` and `Range` of a head; `None` when it is not an HTTP/1 request
/// or names its host twice (which two readers could read differently).
fn parse(head: &[u8]) -> Option<Request<'_>> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut line = lines.next()?.split(' ');
    let (method, target, version) = (line.next()?, line.next()?, line.next()?);
    if line.next().is_some() || !version.starts_with("HTTP/1.") {
        return None;
    }
    let headers: Vec<(&str, &str)> = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim(), value.trim()))
        .collect();
    let named = |wanted: &str| {
        headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| *value)
            .collect::<Vec<_>>()
    };
    let hosts = named("host");
    if hosts.len() > 1 {
        return None;
    }
    Some(Request {
        method,
        target,
        host: hosts.first().copied(),
        range: named("range").first().copied(),
    })
}

struct Reply {
    status: u16,
    reason: &'static str,
    media_type: &'static str,
    body: Vec<u8>,
    head_only: bool,
    /// Header lines of this answer alone, each ending in CRLF.
    extra: String,
}

impl Reply {
    fn plain(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            media_type: "text/plain; charset=utf-8",
            body: reason.as_bytes().to_vec(),
            head_only: false,
            extra: String::new(),
        }
    }
}

async fn reply_to(head: &[u8], site: &Site) -> Reply {
    let Some(request) = parse(head) else {
        return Reply::plain(400, "Bad Request");
    };
    if !request
        .host
        .is_some_and(|host| host_is_page(host, site.port))
    {
        return Reply::plain(403, "Forbidden");
    }
    let head_only = match request.method {
        "GET" => false,
        "HEAD" => true,
        _ => {
            return Reply {
                extra: "Allow: GET, HEAD\r\n".into(),
                ..Reply::plain(405, "Method Not Allowed")
            };
        }
    };
    let Some((token, parts)) = requested_parts(request.target) else {
        return Reply::plain(404, "Not Found");
    };
    if !auth::same_token(&token, &site.token) {
        return Reply::plain(404, "Not Found");
    }
    let root = site.root.clone();
    let named = parts.clone();
    // A read that panicked is a file that is not there.
    let found = tokio::task::spawn_blocking(move || read_below(&root, &named))
        .await
        .unwrap_or(Found::Missing);
    let (path, body) = match found {
        Found::File { path, body } => (path, body),
        Found::TooLarge => return Reply::plain(413, "Content Too Large"),
        Found::Missing => return Reply::plain(404, "Not Found"),
    };
    if path == site.document {
        site.served.loaded(&path);
    } else {
        site.served.gave(&path);
    }
    let media_type = parts.last().map_or("", |name| content_type(name));
    let length = u64::try_from(body.len()).unwrap_or(u64::MAX);
    match byte_range(request.range, length) {
        RangeAnswer::Whole => Reply {
            status: 200,
            reason: "OK",
            media_type,
            body,
            head_only,
            extra: "Accept-Ranges: bytes\r\n".into(),
        },
        RangeAnswer::Part { start, end } => {
            let from = usize::try_from(start).unwrap_or(usize::MAX);
            let to = usize::try_from(end).unwrap_or(usize::MAX);
            Reply {
                status: 206,
                reason: "Partial Content",
                media_type,
                body: body.get(from..=to).unwrap_or_default().to_vec(),
                head_only,
                extra: format!(
                    "Accept-Ranges: bytes\r\nContent-Range: bytes {start}-{end}/{length}\r\n"
                ),
            }
        }
        RangeAnswer::Unsatisfiable => Reply {
            extra: format!("Content-Range: bytes */{length}\r\n"),
            ..Reply::plain(416, "Range Not Satisfiable")
        },
    }
}

enum Found {
    File { path: PathBuf, body: Vec<u8> },
    TooLarge,
    Missing,
}

/// The file `parts` names below `root`, read from its canonical path: a link that leads out of
/// the directory, or into a part the page does not serve, is not there, and neither is anything
/// that is not a regular file. The type is checked before the file is opened, and the file is
/// opened without waiting (a pipe that nobody writes to would hold the thread for ever) and
/// checked again once it is.
fn read_below(root: &Path, parts: &[String]) -> Found {
    let candidate = parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    let Ok(file) = soyeht_platform::fs::canonicalize(&candidate) else {
        return Found::Missing;
    };
    let Ok(relative) = file.strip_prefix(root) else {
        return Found::Missing;
    };
    let named = relative
        .components()
        .all(|part| part.as_os_str().to_str().is_some_and(name_allowed));
    if !named || !std::fs::metadata(&file).is_ok_and(|metadata| metadata.is_file()) {
        return Found::Missing;
    }
    let Ok(open) = open_without_waiting(&file) else {
        return Found::Missing;
    };
    if !open.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Found::Missing;
    }
    let limit = u64::try_from(limits::WEB_PROJECT_FILE_BYTES).unwrap_or(u64::MAX);
    let mut body = Vec::new();
    if open.take(limit + 1).read_to_end(&mut body).is_err() {
        return Found::Missing;
    }
    if u64::try_from(body.len()).map_or(true, |length| length > limit) {
        Found::TooLarge
    } else {
        Found::File { path: file, body }
    }
}

fn open_without_waiting(file: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(file)
}

async fn write_reply(stream: &mut TcpStream, reply: &Reply, policy: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: {}\r\nReferrer-Policy: no-referrer\r\n\
         Access-Control-Allow-Origin: null\r\nConnection: close\r\n\r\n",
        reply.status,
        reply.reason,
        reply.media_type,
        reply.body.len(),
        reply.extra,
        policy,
    );
    stream.write_all(head.as_bytes()).await?;
    if !reply.head_only {
        stream.write_all(&reply.body).await?;
    }
    stream.shutdown().await
}
