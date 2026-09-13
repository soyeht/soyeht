use crate::{Client, Decoder, Incoming, Result, encode};

pub trait TransportPort {
    fn send(&mut self, bytes: &[u8]) -> Result<()>;
    fn receive(&mut self) -> Result<Option<Vec<u8>>>;
}

#[derive(Debug, Default)]
pub struct MemoryTransport {
    pub sent: Vec<Vec<u8>>,
    pub received: Vec<Vec<u8>>,
}

impl TransportPort for MemoryTransport {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.sent.push(bytes.to_vec());
        Ok(())
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>> {
        if self.received.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.received.remove(0)))
        }
    }
}

pub fn drive_once(
    client: &mut Client,
    decoder: &mut Decoder,
    transport: &mut (impl TransportPort + ?Sized),
    outgoing: Option<&serde_json::Value>,
) -> Result<Vec<Incoming>> {
    if let Some(value) = outgoing {
        transport.send(&encode(value)?)?;
    }
    let Some(bytes) = transport.receive()? else {
        return Ok(Vec::new());
    };
    decoder
        .push(&bytes)?
        .into_iter()
        .map(|value| client.accept(value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn memory_transport_proves_framing_and_correlation_without_io() {
        let mut client = Client::default();
        let request = client.initialize("file:///workspace").unwrap();
        let mut transport = MemoryTransport {
            received: vec![encode(&json!({"jsonrpc":"2.0","id":1,"result":{}})).unwrap()],
            ..MemoryTransport::default()
        };
        let incoming = drive_once(
            &mut client,
            &mut Decoder::default(),
            &mut transport,
            Some(&request),
        )
        .unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(transport.sent.len(), 1);
        assert_eq!(client.pending(), 0);
    }
}
