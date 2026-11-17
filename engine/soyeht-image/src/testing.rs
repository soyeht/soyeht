//! What this crate's tests and the command line's tests share (the `test-support` feature): a
//! scratch folder, PNG files made in memory and a stand-in server that records what `curl` sends
//! and answers what the test says. Nothing here is used by the program itself.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// How long the stand-in waits between looks for a new connection, and for a request to arrive.
const IDLE: Duration = Duration::from_millis(10);
const READ_LIMIT: Duration = Duration::from_secs(10);

/// A folder of one test, removed when it goes out of scope.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        Self(soyeht_platform::testing::scratch_dir(name))
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            soyeht_platform::log::failure("image.scratch_remove_failed", &error);
        }
    }
}

/// An RGBA PNG whose pixel at `(x, y)` is `pixel(x, y)`.
pub fn rgba_png(
    width: u32,
    height: u32,
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> Result<Vec<u8>, png::EncodingError> {
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&pixel(x, y));
        }
    }
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(bytes)
}

/// What the stand-in server received.
#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub path: String,
    /// Header names in lower case.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Received {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

type Handler = dyn Fn(&Received) -> (u16, Vec<u8>) + Send + Sync;

/// A server on the loopback. A status of 300 to 399 carries a `Location` header.
pub struct Server {
    pub url: String,
    requests: Arc<Mutex<Vec<Received>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(
        handler: impl Fn(&Received) -> (u16, Vec<u8>) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let listener = soyeht_platform::net::listen(SocketAddr::from(([127, 0, 0, 1], 0)))?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = {
            let (requests, stop) = (requests.clone(), stop.clone());
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => serve(stream, &*handler, &requests),
                        Err(_) => thread::sleep(IDLE),
                    }
                }
            })
        };
        Ok(Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    /// Every request received so far, in order.
    pub fn received(&self) -> Vec<Received> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            soyeht_platform::log::event("image.stand_in_panicked", &[]);
        }
    }
}

fn serve(mut stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<Received>>) {
    if stream.set_nonblocking(false).is_err() || stream.set_read_timeout(Some(READ_LIMIT)).is_err()
    {
        return;
    }
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let (status, body) = handler(&request);
    requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(request);
    let location = if (300..400).contains(&status) {
        "Location: /elsewhere\r\n"
    } else {
        ""
    };
    let head = format!(
        "HTTP/1.1 {status} X\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if stream.write_all(head.as_bytes()).is_ok() {
        // The client may have gone; the stand-in has nothing to do about it.
        let _written = stream.write_all(&body);
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut acknowledged = false;
    loop {
        if let Some(split) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&data[..split]).into_owned();
            let mut lines = head.split("\r\n");
            let mut first = lines.next()?.split(' ');
            let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
            let headers: BTreeMap<String, String> = lines
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim().to_lowercase(), value.trim().to_owned()))
                .collect();
            let length: usize = headers
                .get("content-length")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            if headers.contains_key("expect") && !acknowledged {
                stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").ok()?;
                acknowledged = true;
            }
            if data.len() >= split + 4 + length {
                let body = data[split + 4..split + 4 + length].to_vec();
                return Some(Received {
                    method,
                    path,
                    headers,
                    body,
                });
            }
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..read]);
    }
}
