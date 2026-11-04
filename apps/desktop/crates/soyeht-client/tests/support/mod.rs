//! What the device tests share: a WebSocket server on a loopback port, and reading and sending JSON.

use serde_json::Value;
use std::net::{SocketAddr, TcpListener, TcpStream};
use tungstenite::{Message, WebSocket, accept};

pub type Socket = WebSocket<TcpStream>;

/// Runs `script` on the first connection to a loopback port, after the WebSocket handshake.
pub fn serve<T: Send + 'static>(
    script: impl FnOnce(Socket) -> T + Send + 'static,
) -> (SocketAddr, std::thread::JoinHandle<T>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || script(accept(listener.accept().unwrap().0).unwrap()));
    (address, handle)
}

/// The next JSON message, or none when the connection ended.
pub fn read(socket: &mut Socket) -> Option<Value> {
    loop {
        match socket.read().ok()? {
            Message::Text(text) => return serde_json::from_str(text.as_str()).ok(),
            Message::Close(_) => return None,
            _ => continue,
        }
    }
}

pub fn send(socket: &mut Socket, message: &Value) {
    socket.send(Message::text(message.to_string())).unwrap();
}
