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

/// The refusal of a path that is not a regular file (a directory, a device).
pub(super) fn require_regular_file(metadata: &fs::Metadata) -> io::Result<()> {
    if metadata.is_file() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "file is not a regular file",
    ))
}

/// Whether the regular file at `path` holds exactly the bytes of `expected`. A file of another
/// length is answered from its metadata; otherwise it is compared a chunk at a time and the
/// comparison ends at the first difference.
pub(super) fn file_holds(path: &Path, expected: &str) -> io::Result<bool> {
    // The type is checked before the file is opened: opening a named pipe waits for a writer.
    require_regular_file(&fs::metadata(path)?)?;
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    require_regular_file(&metadata)?;
    if metadata.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut rest = expected.as_bytes();
    let mut chunk = vec![0; CHUNK_BYTES.min(rest.len().max(1))];
    while !rest.is_empty() {
        let wanted = chunk.len().min(rest.len());
        let read = match file.read(&mut chunk[..wanted]) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 || chunk[..read] != rest[..read] {
            // A file that ended early is a file that changed while it was read.
            return Ok(false);
        }
        rest = &rest[read..];
    }
    // The file grew while it was read.
    loop {
        match file.read(&mut chunk[..1]) {
            Ok(read) => return Ok(read == 0),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error),
        }
    }
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

    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_refused_and_not_waited_for() {
        use std::{sync::mpsc, thread, time::Duration};
        let path = std::env::temp_dir().join(format!("soyeht-compare-{}", Uuid::new_v4()));
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let (sender, receiver) = mpsc::channel();
        let asked = path.clone();
        // Opening a pipe nobody writes to never returns, so the question is asked off this
        // thread and given a deadline.
        thread::spawn(move || {
            let _ = sender.send(file_holds(&asked, "text"));
        });
        let answer = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(answer.unwrap_err().kind(), io::ErrorKind::InvalidData);
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
