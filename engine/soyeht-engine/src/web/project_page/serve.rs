//! The server of one project page: HTTP/1.1 over a loopback socket, `GET` and `HEAD` of the files
//! below the page's directory, one request per connection. What a request may name is decided by
//! `soyeht_web::requested_parts`; what is read is decided here, on the canonical path, on every
//! request.

use soyeht_protocol::limits;
use soyeht_web::{CONTENT_SECURITY_POLICY, content_type, host_is_page, requested_parts};
use std::{io::Read, path::PathBuf, sync::Arc, time::Duration};
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
}

impl Site {
    pub(super) fn new(port: u16, token: String, root: PathBuf) -> Self {
        Self { port, token, root }
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
    drop(timeout(WRITE_PATIENCE, write_reply(&mut stream, &reply)).await);
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
}

/// The request line and the `Host` of a head; `None` when it is not an HTTP/1 request or names its
/// host twice (which two readers could read differently).
fn parse(head: &[u8]) -> Option<Request<'_>> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut line = lines.next()?.split(' ');
    let (method, target, version) = (line.next()?, line.next()?, line.next()?);
    if line.next().is_some() || !version.starts_with("HTTP/1.") {
        return None;
    }
    let mut hosts = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim());
    let host = hosts.next();
    if hosts.next().is_some() {
        return None;
    }
    Some(Request {
        method,
        target,
        host,
    })
}

struct Reply {
    status: u16,
    reason: &'static str,
    media_type: &'static str,
    body: Vec<u8>,
    head_only: bool,
}

impl Reply {
    fn plain(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            media_type: "text/plain; charset=utf-8",
            body: reason.as_bytes().to_vec(),
            head_only: false,
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
        _ => return Reply::plain(405, "Method Not Allowed"),
    };
    let Some(parts) = requested_parts(&site.token, request.target) else {
        return Reply::plain(404, "Not Found");
    };
    let root = site.root.clone();
    let named = parts.clone();
    // A read that panicked is a file that is not there.
    let found = tokio::task::spawn_blocking(move || read_below(&root, &named))
        .await
        .unwrap_or(Found::Missing);
    match found {
        Found::File(body) => Reply {
            status: 200,
            reason: "OK",
            media_type: parts.last().map_or("", |name| content_type(name)),
            body,
            head_only,
        },
        Found::TooLarge => Reply::plain(413, "Content Too Large"),
        Found::Missing => Reply::plain(404, "Not Found"),
    }
}

enum Found {
    File(Vec<u8>),
    TooLarge,
    Missing,
}

/// The file `parts` names below `root`, read from its canonical path: a link that leads out of
/// the directory, or into a hidden part, is not there.
fn read_below(root: &std::path::Path, parts: &[String]) -> Found {
    let candidate = parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    let Ok(file) = soyeht_platform::fs::canonicalize(&candidate) else {
        return Found::Missing;
    };
    let Ok(relative) = file.strip_prefix(root) else {
        return Found::Missing;
    };
    if relative.components().any(super::hidden) {
        return Found::Missing;
    }
    let Ok(open) = std::fs::File::open(&file) else {
        return Found::Missing;
    };
    match open.metadata() {
        Ok(metadata) if metadata.is_file() => {}
        _ => return Found::Missing,
    }
    let limit = u64::try_from(limits::WEB_PROJECT_FILE_BYTES).unwrap_or(u64::MAX);
    let mut body = Vec::new();
    if open.take(limit + 1).read_to_end(&mut body).is_err() {
        return Found::Missing;
    }
    if u64::try_from(body.len()).map_or(true, |len| len > limit) {
        Found::TooLarge
    } else {
        Found::File(body)
    }
}

async fn write_reply(stream: &mut TcpStream, reply: &Reply) -> std::io::Result<()> {
    let allow = if reply.status == 405 {
        "Allow: GET, HEAD\r\n"
    } else {
        ""
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{allow}\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: {CONTENT_SECURITY_POLICY}\r\nReferrer-Policy: no-referrer\r\n\
         Access-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        reply.status,
        reply.reason,
        reply.media_type,
        reply.body.len(),
    );
    stream.write_all(head.as_bytes()).await?;
    if !reply.head_only {
        stream.write_all(&reply.body).await?;
    }
    stream.shutdown().await
}
