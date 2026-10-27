use crate::{AttachmentRef, Error, Result, frame::Frame, limits};
use uuid::Uuid;

/// Collects type-3 frames until the next JSON request commits them. A transfer
/// over the byte or count limit is discarded and reported by that request, so
/// the sender receives an error response instead of a closed connection.
#[derive(Debug)]
pub struct AttachmentAssembler {
    maximum_bytes: usize,
    active_id: Option<[u8; 16]>,
    bytes: Vec<u8>,
    completed: Vec<([u8; 16], Vec<u8>)>,
    last_completed_id: Option<[u8; 16]>,
    overflowed: bool,
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
            completed: Vec::new(),
            last_completed_id: None,
            overflowed: false,
        })
    }

    pub fn push(&mut self, header: [u8; 16], chunk: &[u8]) -> Result<()> {
        if self.overflowed {
            return Ok(());
        }
        if chunk.is_empty()
            || self.last_completed_id == Some(header)
            || self.completed.iter().any(|(id, _)| *id == header)
        {
            return Err(Error::DuplicateAttachment);
        }
        if self.active_id.is_some_and(|active| active != header) {
            self.finish_active();
        }
        let length = self
            .completed
            .iter()
            .try_fold(self.bytes.len(), |total, (_, bytes)| {
                total.checked_add(bytes.len())
            })
            .and_then(|length| length.checked_add(chunk.len()));
        if self.overflowed || length.is_none_or(|length| length > self.maximum_bytes) {
            self.overflow();
            return Ok(());
        }
        self.active_id = Some(header);
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    fn finish_active(&mut self) {
        let Some(id) = self.active_id.take() else {
            return;
        };
        if self.completed.len() >= limits::PROMPT_ATTACHMENTS {
            self.overflow();
            return;
        }
        self.completed.push((id, std::mem::take(&mut self.bytes)));
    }

    fn overflow(&mut self) {
        self.reset();
        self.overflowed = true;
    }

    fn reset(&mut self) {
        self.active_id = None;
        self.bytes = Vec::new();
        self.completed = Vec::new();
        self.overflowed = false;
    }

    /// Drops what has been collected and not committed (the frames of an answer nobody waits for),
    /// so the next request finds the assembler empty. A transfer already committed is still
    /// recognised as a duplicate.
    pub fn discard(&mut self) {
        self.finish_active();
        self.reset();
    }

    /// Hands the collected transfers to the request that names them. Every
    /// outcome, success or error, leaves the assembler empty for the next one.
    pub fn commit_all(&mut self, references: &[AttachmentRef]) -> Result<Vec<Vec<u8>>> {
        let result = self.take(references);
        self.reset();
        result
    }

    fn take(&mut self, references: &[AttachmentRef]) -> Result<Vec<Vec<u8>>> {
        self.finish_active();
        if self.overflowed {
            return Err(Error::AttachmentTooLarge);
        }
        if references.iter().try_fold(false, |duplicate, reference| {
            Ok::<_, Error>(
                duplicate
                    || self.last_completed_id == Some(attachment_header(&reference.attachment_id)?),
            )
        })? {
            return Err(Error::DuplicateAttachment);
        }
        if references.len() != self.completed.len() {
            return Err(Error::UnknownAttachment);
        }
        let mut output = Vec::with_capacity(references.len());
        for (reference, (id, bytes)) in references.iter().zip(&mut self.completed) {
            let expected_id = attachment_header(&reference.attachment_id)?;
            if expected_id != *id {
                return Err(Error::UnknownAttachment);
            }
            if decimal_bytes(&reference.bytes)? != bytes.len() {
                return Err(Error::IncompleteAttachment);
            }
            output.push(std::mem::take(bytes));
        }
        self.last_completed_id = self.completed.last().map(|(id, _)| *id);
        Ok(output)
    }

    pub fn commit(&mut self, reference: &AttachmentRef) -> Result<Vec<u8>> {
        self.commit_all(std::slice::from_ref(reference))?
            .pop()
            .ok_or(Error::UnknownAttachment)
    }

    pub fn is_pending(&self) -> bool {
        self.overflowed || self.active_id.is_some() || !self.completed.is_empty()
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
    fn a_discarded_transfer_leaves_the_next_request_an_empty_assembler() {
        let (stale, fresh) = (
            "10213243-5465-4768-99aa-bbccddeeff00",
            "00112233-4455-4677-8899-aabbccddeeff",
        );
        let mut input = AttachmentAssembler::new(8).unwrap();
        input
            .push(attachment_header(stale).unwrap(), b"late")
            .unwrap();
        input.discard();
        assert!(!input.is_pending());
        input
            .push(attachment_header(fresh).unwrap(), b"next")
            .unwrap();
        assert_eq!(input.commit(&reference(fresh, 4)).unwrap(), b"next");
    }

    #[test]
    fn input_rejects_oversize_unknown_and_incomplete_transfers() {
        let first = "00112233-4455-4677-8899-aabbccddeeff";
        let second = "10213243-5465-4768-99aa-bbccddeeff00";
        let mut input = AttachmentAssembler::new(4).unwrap();
        input
            .push(attachment_header(first).unwrap(), b"ab")
            .unwrap();
        input
            .push(attachment_header(second).unwrap(), b"c")
            .unwrap();
        assert!(matches!(
            input.commit_all(&[reference(second, 1), reference(first, 2)]),
            Err(Error::UnknownAttachment)
        ));
        assert!(!input.is_pending(), "a failed commit leaves nothing behind");

        let mut oversize = AttachmentAssembler::new(4).unwrap();
        oversize
            .push(attachment_header(first).unwrap(), b"ab")
            .unwrap();
        oversize
            .push(attachment_header(second).unwrap(), b"cde")
            .unwrap();
        assert!(oversize.is_pending(), "the overflow waits for its request");
        assert!(matches!(
            oversize.commit_all(&[reference(first, 2), reference(second, 3)]),
            Err(Error::AttachmentTooLarge)
        ));
        oversize
            .push(attachment_header(second).unwrap(), b"cd")
            .unwrap();
        assert_eq!(oversize.commit(&reference(second, 2)).unwrap(), b"cd");
        let mut incomplete = AttachmentAssembler::new(4).unwrap();
        incomplete
            .push(attachment_header(first).unwrap(), b"ab")
            .unwrap();
        assert!(matches!(
            incomplete.commit(&reference(first, 3)),
            Err(Error::IncompleteAttachment)
        ));
    }

    #[test]
    fn one_json_message_commits_an_ordered_attachment_group() {
        let first = reference("00112233-4455-4677-8899-aabbccddeeff", 3);
        let second = reference("10213243-5465-4768-99aa-bbccddeeff00", 2);
        let mut input = AttachmentAssembler::new(5).unwrap();
        input
            .push(attachment_header(&first.attachment_id).unwrap(), b"abc")
            .unwrap();
        input
            .push(attachment_header(&second.attachment_id).unwrap(), b"de")
            .unwrap();
        assert_eq!(
            input.commit_all(&[first, second]).unwrap(),
            vec![b"abc".to_vec(), b"de".to_vec()]
        );
        assert!(!input.is_pending());
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
