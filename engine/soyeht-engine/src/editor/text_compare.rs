//! Whether a file holds exactly a text, without reading the file into memory. The editor asks it
//! of every open file several times a second, and a copy of a large file for each ask would
//! allocate and free as much memory as the file is big.

use std::{
    fs,
    io::{self, Read},
    path::Path,
};

/// How much of the file is read at a time.
const CHUNK_BYTES: usize = 64 * 1024;

/// Whether the regular file at `path` holds exactly the bytes of `expected`. A file of another
/// length is answered from its metadata; otherwise it is compared a chunk at a time and the
/// comparison ends at the first difference.
pub(super) fn file_holds(path: &Path, expected: &str) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file is not a regular file",
        ));
    }
    if metadata.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut rest = expected.as_bytes();
    let mut chunk = vec![0; CHUNK_BYTES.min(rest.len().max(1))];
    while !rest.is_empty() {
        let wanted = chunk.len().min(rest.len());
        let read = file.read(&mut chunk[..wanted])?;
        if read == 0 || chunk[..read] != rest[..read] {
            // A file that ended early is a file that changed while it was read.
            return Ok(false);
        }
        rest = &rest[read..];
    }
    // The file grew while it was read.
    Ok(file.read(&mut chunk[..1])? == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn file_with(text: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("soyeht-compare-{}", Uuid::new_v4()));
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_file_holds_a_text_only_when_every_byte_is_the_same() {
        let long = "line of text\n".repeat(CHUNK_BYTES / 4);
        let path = file_with(long.as_bytes());
        assert!(file_holds(&path, &long).unwrap());
        // One byte different, at the start, in the middle and in the last chunk.
        for at in [0, long.len() / 2, long.len() - 1] {
            let mut other = long.clone().into_bytes();
            other[at] = b'#';
            assert!(!file_holds(&path, std::str::from_utf8(&other).unwrap()).unwrap());
        }
        // A longer or shorter text is another file.
        assert!(!file_holds(&path, &format!("{long}!")).unwrap());
        assert!(!file_holds(&path, &long[1..]).unwrap());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_empty_file_holds_the_empty_text_and_a_missing_or_directory_path_is_an_error() {
        let path = file_with(b"");
        assert!(file_holds(&path, "").unwrap());
        assert!(!file_holds(&path, "x").unwrap());
        fs::remove_file(&path).unwrap();
        assert_eq!(
            file_holds(&path, "").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(file_holds(&std::env::temp_dir(), "").is_err());
    }
}
