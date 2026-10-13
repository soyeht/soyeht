use crate::errors::{Error, Result};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use soyeht_platform::local::asynchronous::{OwnedReadHalf, OwnedWriteHalf, Stream};
use soyeht_protocol::{
    frame::{Frame, validate_length},
    limits,
};
use std::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{
    WebSocketStream, accept_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

type WebSocket = WebSocketStream<TcpStream>;

pub enum Reader {
    Local(OwnedReadHalf),
    Web(SplitStream<WebSocket>),
}
pub enum Writer {
    Local(OwnedWriteHalf),
    Web(SplitSink<WebSocket, Message>),
}

pub fn local(stream: Stream) -> (Reader, Writer) {
    let (read, write) = stream.into_split();
    (Reader::Local(read), Writer::Local(write))
}

pub async fn websocket(stream: TcpStream) -> Result<(Reader, Writer)> {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(limits::JSON_BYTES);
    config.max_frame_size = Some(limits::JSON_BYTES);
    config.max_write_buffer_size = limits::CONNECTION_QUEUE_BYTES;
    let socket = accept_async_with_config(stream, Some(config)).await?;
    let (write, read) = socket.split();
    Ok((Reader::Web(read), Writer::Web(write)))
}

impl Reader {
    pub async fn next(&mut self) -> Result<Option<Frame>> {
        match self {
            Self::Local(reader) => {
                let kind = match reader.read_u8().await {
                    Ok(kind) => kind,
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
                    Err(error) => return Err(error.into()),
                };
                let length = reader.read_u32().await?;
                validate_length(kind, length as usize)?;
                let mut wire = vec![0; 5 + length as usize];
                wire[0] = kind;
                wire[1..5].copy_from_slice(&length.to_be_bytes());
                reader.read_exact(&mut wire[5..]).await?;
                Ok(Frame::read_from(&wire[..])?)
            }
            Self::Web(reader) => {
                while let Some(message) = reader.next().await {
                    match message? {
                        Message::Text(text) => {
                            validate_length(limits::FRAME_KIND_JSON, text.len())?;
                            return Ok(Some(Frame::Json(text.as_bytes().to_vec())));
                        }
                        Message::Binary(bytes) => {
                            let mut cursor = io::Cursor::new(bytes.as_ref());
                            let frame = Frame::read_from(&mut cursor)?;
                            if cursor.position() != bytes.len() as u64
                                || matches!(frame, Some(Frame::Json(_)))
                            {
                                return Err(Error::InvalidBinaryWebSocketFrame);
                            }
                            return Ok(frame);
                        }
                        Message::Close(_) => return Ok(None),
                        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => (),
                    }
                }
                Ok(None)
            }
        }
    }
}

impl Writer {
    pub async fn write(&mut self, frame: &Frame) -> Result<()> {
        match self {
            Self::Local(writer) => {
                let mut wire = Vec::new();
                frame.write_to(&mut wire)?;
                writer.write_all(&wire).await?;
                Ok(())
            }
            Self::Web(writer) => {
                let message = match frame {
                    Frame::Json(bytes) => {
                        validate_length(limits::FRAME_KIND_JSON, bytes.len())?;
                        Message::Text(String::from_utf8(bytes.clone())?.into())
                    }
                    _ => {
                        let mut bytes = Vec::new();
                        frame.write_to(&mut bytes)?;
                        Message::Binary(bytes.into())
                    }
                };
                writer.send(message).await?;
                Ok(())
            }
        }
    }
    pub async fn flush(&mut self) -> Result<()> {
        match self {
            Self::Local(writer) => {
                writer.flush().await?;
                Ok(())
            }
            Self::Web(writer) => {
                writer.flush().await?;
                Ok(())
            }
        }
    }
    /// Best effort: the connection is over either way, and the close reason is already set.
    pub async fn close(&mut self) {
        match self {
            Self::Local(writer) => {
                let _ = writer.shutdown().await;
            }
            Self::Web(writer) => {
                let _ = writer.close().await;
            }
        }
    }
}
