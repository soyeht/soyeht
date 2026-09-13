use crate::{AttachmentRef, Error, Result, frame::Frame, limits};
use uuid::Uuid;

#[derive(Debug)]
pub struct AttachmentAssembler {
    maximum_bytes: usize,
    active_id: Option<[u8; 16]>,
    bytes: Vec<u8>,
    last_completed_id: Option<[u8; 16]>,
}

impl AttachmentAssembler {
    pub fn new(maximum_bytes: usize) -> Result<Self> {
        if maximum_bytes == 0 {
            return Err(Error::InvalidAttachmentLimit);
        }
        Ok(Self {
            maximum_bytes,
            active_id: None,
            bytes: Vec::new(),
            last_completed_id: None,
        })
    }

    pub fn push(&mut self, header: [u8; 16], chunk: &[u8]) -> Result<()> {
        if chunk.is_empty() || self.last_completed_id == Some(header) {
            return Err(Error::DuplicateAttachment);
        }
        if self.active_id.is_some_and(|active| active != header) {
            return Err(Error::InterleavedAttachment);
        }
        let length = self
            .bytes
            .len()
            .checked_add(chunk.len())
            .ok_or(Error::AttachmentTooLarge)?;
        if length > self.maximum_bytes {
            return Err(Error::AttachmentTooLarge);
        }
        self.active_id = Some(header);
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    pub fn commit(&mut self, reference: &AttachmentRef) -> Result<Vec<u8>> {
        let expected_id = attachment_header(&reference.attachment_id)?;
        if self.last_completed_id == Some(expected_id) {
            return Err(Error::DuplicateAttachment);
        }
        if self.active_id != Some(expected_id) {
            return Err(Error::UnknownAttachment);
        }
        let declared = decimal_bytes(&reference.bytes)?;
        if declared != self.bytes.len() {
            return Err(Error::IncompleteAttachment);
        }
        self.active_id = None;
        self.last_completed_id = Some(expected_id);
        Ok(std::mem::take(&mut self.bytes))
    }

    pub fn is_pending(&self) -> bool {
        self.active_id.is_some()
    }
}

pub fn attachment_frames(reference: &AttachmentRef, bytes: &[u8]) -> Result<Vec<Frame>> {
    let header = attachment_header(&reference.attachment_id)?;
    if bytes.is_empty() || decimal_bytes(&reference.bytes)? != bytes.len() {
        return Err(Error::IncompleteAttachment);
    }
    Ok(bytes
        .chunks(limits::ATTACHMENT_PAYLOAD_BYTES)
        .map(|chunk| Frame::Attachment {
            header,
            bytes: chunk.to_vec(),
        })
        .collect())
}

pub fn attachment_header(id: &str) -> Result<[u8; 16]> {
    let uuid = Uuid::parse_str(id).map_err(|_| Error::InvalidAttachmentId)?;
    if uuid.hyphenated().to_string() != id {
        return Err(Error::InvalidAttachmentId);
    }
    Ok(*uuid.as_bytes())
}

fn decimal_bytes(value: &str) -> Result<usize> {
    if value.is_empty() || value.starts_with('0') {
        return Err(Error::InvalidAttachmentSize);
    }
    value
        .parse::<usize>()
        .map_err(|_| Error::InvalidAttachmentSize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(id: &str, bytes: usize) -> AttachmentRef {
        AttachmentRef {
            attachment_id: id.into(),
            mime_type: "image/png".into(),
            name: "capture.png".into(),
            bytes: bytes.to_string(),
        }
    }

    #[test]
    fn output_is_chunked_and_input_commits_only_at_the_exact_declared_size() {
        let id = "00112233-4455-4677-8899-aabbccddeeff";
        let data = vec![7; limits::ATTACHMENT_PAYLOAD_BYTES + 17];
        let reference = reference(id, data.len());
        let frames = attachment_frames(&reference, &data).unwrap();
        assert_eq!(frames.len(), 2);

        let mut input = AttachmentAssembler::new(data.len()).unwrap();
        for frame in frames {
            let Frame::Attachment { header, bytes } = frame else {
                panic!("attachment frame expected");
            };
            input.push(header, &bytes).unwrap();
        }
        assert_eq!(input.commit(&reference).unwrap(), data);
        assert!(matches!(
            input.commit(&reference),
            Err(Error::DuplicateAttachment)
        ));
    }

    #[test]
    fn input_rejects_interleaving_oversize_unknown_and_incomplete_transfers() {
        let first = "00112233-4455-4677-8899-aabbccddeeff";
        let second = "10213243-5465-4768-99aa-bbccddeeff00";
        let mut input = AttachmentAssembler::new(4).unwrap();
        input
            .push(attachment_header(first).unwrap(), b"ab")
            .unwrap();
        assert!(matches!(
            input.push(attachment_header(second).unwrap(), b"c"),
            Err(Error::InterleavedAttachment)
        ));
        assert!(matches!(
            input.push(attachment_header(first).unwrap(), b"cde"),
            Err(Error::AttachmentTooLarge)
        ));
        assert!(matches!(
            input.commit(&reference(second, 2)),
            Err(Error::UnknownAttachment)
        ));
        assert!(matches!(
            input.commit(&reference(first, 3)),
            Err(Error::IncompleteAttachment)
        ));
    }

    #[test]
    fn ids_and_decimal_sizes_use_one_canonical_spelling() {
        let id = "00112233-4455-4677-8899-aabbccddeeff";
        assert_eq!(
            attachment_header(id).unwrap(),
            Uuid::parse_str(id).unwrap().into_bytes()
        );
        for invalid in [
            "00112233445546778899aabbccddeeff",
            "00112233-4455-4677-8899-AABBCCDDEEFF",
            "not-an-id",
        ] {
            assert!(matches!(
                attachment_header(invalid),
                Err(Error::InvalidAttachmentId)
            ));
        }
        for size in ["", "0", "01", "-1", "one"] {
            assert!(matches!(
                decimal_bytes(size),
                Err(Error::InvalidAttachmentSize)
            ));
        }
    }
}
