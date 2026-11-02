//! The engine's WebSocket as a [`Link`]. The engine sends a JSON frame as a text message and every
//! other frame as a binary message that keeps the common header (`transport.rs` of the engine), so
//! the rest of the client reads and writes the same frames it does on the local socket.
//!
//! One thread owns the socket: it reads with a short timeout and, between reads, sends what the
//! writer queued. The protocol library cannot split a socket into a reading and a writing half.

use crate::{Error, Result, transport::Link};
use soyeht_protocol::{frame::Frame, limits};
use std::{
    io::{self, Cursor, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tungstenite::{Message, protocol::WebSocketConfig};

/// How long the socket thread waits for the engine before it looks at what is queued to send. It is
/// the delay a keystroke can wait; the thread sleeps in the kernel between reads.
const POLL: Duration = Duration::from_millis(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Connects and opens the WebSocket.
pub(crate) fn connect(address: SocketAddr) -> Result<Link> {
    let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
    stream.set_nodelay(true)?;
    let shutdown = stream.try_clone()?;
    let largest = (limits::ATTACHMENT_HEADER_BYTES + limits::ATTACHMENT_PAYLOAD_BYTES + 5)
        .max(limits::JSON_BYTES);
    let config = WebSocketConfig::default()
        .max_message_size(Some(largest))
        .max_frame_size(Some(largest));
    let (mut socket, _) =
        tungstenite::client::client_with_config(format!("ws://{address}/"), stream, Some(config))
            .map_err(|_| Error::Disconnected)?;
    socket.get_ref().set_read_timeout(Some(POLL))?;
    let (incoming, inbox) = mpsc::channel::<Vec<u8>>();
    let (outgoing, outbox) = mpsc::channel::<Message>();
    let closed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&closed);
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            if outbox
                .try_iter()
                .any(|message| socket.send(message).is_err())
            {
                break;
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let mut wire = Vec::with_capacity(text.len() + 5);
                    if Frame::Json(text.as_bytes().to_vec())
                        .write_to(&mut wire)
                        .is_err()
                        || incoming.send(wire).is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Binary(bytes)) => {
                    if incoming.send(bytes.to_vec()).is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                // The library answers a ping on the next send.
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                // ignored: a connection that broke ends the reader with a clean end of stream, and the
                // supervisor reconnects; the reason is in how the connection began or ended.
                Err(_) => break,
            }
        }
        // Dropping `incoming` ends the reader with a clean end of stream.
    });
    Ok(Link {
        reader: Box::new(Inbox {
            inbox,
            current: Cursor::new(Vec::new()),
        }),
        writer: Box::new(Outbox {
            outgoing,
            pending: Vec::new(),
        }),
        close: Arc::new(move || {
            closed.store(true, Ordering::Relaxed);
            // A socket that is already closed has nothing left to shut down.
            if let Err(error) = shutdown.shutdown(std::net::Shutdown::Both)
                && error.kind() != io::ErrorKind::NotConnected
            {
                soyeht_platform::log::failure("android.engine_close_failed", &error);
            }
        }),
    })
}

/// The frames the engine sent, as the bytes of the local framing.
struct Inbox {
    inbox: mpsc::Receiver<Vec<u8>>,
    current: Cursor<Vec<u8>>,
}

impl Read for Inbox {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            let read = self.current.read(buffer)?;
            if read > 0 || buffer.is_empty() {
                return Ok(read);
            }
            match self.inbox.recv() {
                Ok(next) => self.current = Cursor::new(next),
                // ignored: the socket thread ended, which is the end of the stream.
                Err(_) => return Ok(0),
            }
        }
    }
}

/// Collects the bytes of whole frames and queues each as a message when flushed.
struct Outbox {
    outgoing: mpsc::Sender<Message>,
    pending: Vec<u8>,
}

impl Write for Outbox {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let pending = std::mem::take(&mut self.pending);
        let mut cursor = Cursor::new(pending.as_slice());
        let gone = || io::Error::new(io::ErrorKind::BrokenPipe, "the connection is closed");
        while cursor.position() < pending.len() as u64 {
            let start = cursor.position() as usize;
            let frame = Frame::read_from(&mut cursor)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            let message = match frame {
                Some(Frame::Json(bytes)) => Message::text(
                    String::from_utf8(bytes)
                        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?,
                ),
                Some(_) => Message::binary(pending[start..cursor.position() as usize].to_vec()),
                None => break,
            };
            self.outgoing.send(message).map_err(|_| gone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A server that answers a WebSocket handshake, sends `sent`, and returns the first `expect`
    /// messages it receives.
    fn serve(
        sent: Vec<Message>,
        expect: usize,
    ) -> (SocketAddr, std::thread::JoinHandle<Vec<Message>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            for message in sent {
                socket.send(message).unwrap();
            }
            let mut received = Vec::new();
            while received.len() < expect {
                match socket.read() {
                    Ok(message) if !message.is_close() => received.push(message),
                    _ => break,
                }
            }
            received
        });
        (address, handle)
    }

    #[test]
    fn json_is_a_text_message_and_the_other_frames_keep_their_header() {
        let terminal = Frame::Terminal {
            pane_ref: 3,
            generation: 1,
            offset: 9,
            bytes: b"hi".to_vec(),
        };
        let mut binary = Vec::new();
        terminal.write_to(&mut binary).unwrap();
        let (address, server) = serve(
            vec![Message::text(r#"{"method":"x"}"#), Message::binary(binary)],
            2,
        );
        let mut link = connect(address).unwrap();
        assert_eq!(
            Frame::read_from(&mut link.reader).unwrap(),
            Some(Frame::Json(br#"{"method":"x"}"#.to_vec()))
        );
        assert_eq!(
            Frame::read_from(&mut link.reader).unwrap(),
            Some(terminal.clone())
        );
        Frame::Json(br#"{"id":1}"#.to_vec())
            .write_to(&mut link.writer)
            .unwrap();
        terminal.write_to(&mut link.writer).unwrap();
        link.writer.flush().unwrap();
        let received = server.join().unwrap();
        (link.close)();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0], Message::text(r#"{"id":1}"#));
        assert!(
            matches!(&received[1], Message::Binary(bytes) if Frame::read_from(&bytes[..]).unwrap() == Some(terminal))
        );
    }

    #[test]
    fn a_closed_connection_ends_the_reader() {
        let (address, server) = serve(Vec::new(), 0);
        let mut link = connect(address).unwrap();
        (link.close)();
        assert_eq!(Frame::read_from(&mut link.reader).unwrap(), None);
        server.join().unwrap();
    }
}
