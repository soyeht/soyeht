//! What the tests share: PNG files made in memory, a scratch folder, and a stand-in server
//! that records what `curl` sends and answers what the test says.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use uuid::Uuid;

/// A folder that is removed when it goes out of scope.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!("soyeht-image-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An RGBA PNG whose pixel at `(x, y)` is `pixel(x, y)`.
pub fn rgba_png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&pixel(x, y));
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&data)
        .unwrap();
    out
}

pub fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
}

/// What the stand-in server received.
#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Received {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

type Handler = dyn Fn(&Received) -> (u16, Vec<u8>) + Send + Sync;

pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Received>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(handler: impl Fn(&Received) -> (u16, Vec<u8>) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = {
            let (requests, stop) = (requests.clone(), stop.clone());
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => serve(stream, &*handler, &requests),
                        Err(_) => thread::sleep(Duration::from_millis(10)),
                    }
                }
            })
        };
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    pub fn received(&self) -> Vec<Received> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(mut stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<Received>>) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let (status, body) = handler(&request);
    requests.lock().unwrap().push(request);
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}

fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut acknowledged = false;
    loop {
        let split = data.windows(4).position(|window| window == b"\r\n\r\n");
        if let Some(split) = split {
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
