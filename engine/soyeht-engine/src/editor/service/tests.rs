use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;
use {super::*, crate::test_paths::rooted};

#[derive(Default)]
struct FakeFiles {
    canonical: BTreeMap<PathBuf, PathBuf>,
    contents: BTreeMap<PathBuf, String>,
    writes: Arc<Mutex<Vec<(PathBuf, String)>>>,
}

impl FileSystemPort for FakeFiles {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.canonical
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "fixture path"))
    }
    fn project_files(&self, _root: &Path) -> io::Result<Vec<ProjectFile>> {
        Ok(Vec::new())
    }
    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String> {
        let text = self
            .contents
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "fixture file"))?;
        (text.len() <= limit)
            .then_some(text)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "fixture too large"))
    }
    fn read_binary(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let bytes = self
            .contents
            .get(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "fixture file"))?
            .as_bytes()
            .to_vec();
        (bytes.len() <= limit)
            .then_some(bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "fixture too large"))
    }
    fn write_text_if_unchanged(
        &self,
        path: &Path,
        _expected: &str,
        text: &str,
    ) -> io::Result<bool> {
        self.writes
            .lock()
            .map_err(|_| io::Error::other("fixture lock"))?
            .push((path.into(), text.into()));
        Ok(true)
    }
    fn creation_target_exists(&self, _root: &Path, relative: &Path) -> io::Result<bool> {
        Ok(self.contents.contains_key(relative))
    }
    fn create_empty_text(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        Ok(root.join(relative))
    }
    fn create_binary_if_absent_or_matching(
        &self,
        root: &Path,
        relative: &Path,
        _bytes: &[u8],
    ) -> io::Result<PathBuf> {
        Ok(root.join(relative))
    }
}

struct SwappingFiles {
    root: PathBuf,
    target: PathBuf,
    outside: PathBuf,
    target_resolutions: AtomicUsize,
    wrote: Arc<AtomicBool>,
}

impl FileSystemPort for SwappingFiles {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        if path == self.root {
            return Ok(self.root.clone());
        }
        if path == self.root.join("note.txt") {
            let call = self.target_resolutions.fetch_add(1, Ordering::SeqCst);
            return Ok(if call == 0 {
                self.target.clone()
            } else {
                self.outside.clone()
            });
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "fixture path"))
    }

    fn project_files(&self, _root: &Path) -> io::Result<Vec<ProjectFile>> {
        Ok(Vec::new())
    }

    fn read_text(&self, _path: &Path, _limit: usize) -> io::Result<String> {
        Ok("before".into())
    }

    fn read_binary(&self, _path: &Path, _limit: usize) -> io::Result<Vec<u8>> {
        Ok(b"before".to_vec())
    }

    fn write_text_if_unchanged(
        &self,
        _path: &Path,
        _expected: &str,
        _text: &str,
    ) -> io::Result<bool> {
        self.wrote.store(true, Ordering::SeqCst);
        Ok(true)
    }
    fn creation_target_exists(&self, _root: &Path, _relative: &Path) -> io::Result<bool> {
        Ok(false)
    }
    fn create_empty_text(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        Ok(root.join(relative))
    }
    fn create_binary_if_absent_or_matching(
        &self,
        root: &Path,
        relative: &Path,
        _bytes: &[u8],
    ) -> io::Result<PathBuf> {
        Ok(root.join(relative))
    }
}

#[test]
fn resolver_rejects_absolute_parent_and_symlink_escape_before_opening() {
    let root = rooted("/repo");
    let mut files = FakeFiles::default();
    files.canonical.insert(root.clone(), root.clone());
    files
        .canonical
        .insert(root.join("src/main.rs"), root.join("src/main.rs"));
    files
        .canonical
        .insert(root.join("link.rs"), rooted("/outside/secret.rs"));
    let service = Service::new(Box::new(files));

    let resolved = service.resolve(&root, "src/main.rs").unwrap();
    assert_eq!(resolved.canonical, root.join("src/main.rs"));
    assert_eq!(resolved.relative, "src/main.rs");
    let absolute = service
        .resolve_absolute(&root, &root.join("src/main.rs"))
        .unwrap();
    assert_eq!(absolute.relative, "src/main.rs");
    assert!(service.resolve(&root, "/outside/secret.rs").is_err());
    assert!(service.resolve(&root, "../secret.rs").is_err());
    assert!(service.resolve(&root, "link.rs").is_err());
    assert!(
        service
            .resolve_absolute(&root, &rooted("/outside/secret.rs"))
            .is_err()
    );
    assert!(
        service
            .resolve_absolute(&root, &root.join("link.rs"))
            .is_err()
    );
}

#[test]
fn conditional_write_re_resolves_and_rejects_a_swapped_symlink_target() {
    let root = rooted("/repo");
    let wrote = Arc::new(AtomicBool::new(false));
    let service = Service::new(Box::new(SwappingFiles {
        root: root.clone(),
        target: root.join("note.txt"),
        outside: rooted("/outside/secret.txt"),
        target_resolutions: AtomicUsize::new(0),
        wrote: wrote.clone(),
    }));
    let file = EditorFile {
        file_id: "file-1".into(),
        pane_id: None,
        workspace_id: "workspace-1".into(),
        relative_path: "note.txt".into(),
        language: EditorLanguage::PlainText,
        revision: 0,
        disk_revision: 0,
        disk_text: "before".into(),
        draft_text: None,
        draft_author: None,
        conflict_text: None,
        watching: false,
        updated_ms: 0,
    };

    assert_eq!(service.current_disk(&root, &file).unwrap(), "before");
    assert!(service.save_disk(&root, &file, "before", "after").is_err());
    assert!(!wrote.load(Ordering::SeqCst));
}

#[test]
fn document_creation_is_exclusive_recoverable_and_symlink_safe() {
    let root = std::env::temp_dir().join(format!("sn-create-{}", Uuid::new_v4()));
    let outside = std::env::temp_dir().join(format!("sn-create-outside-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    let files = SystemFiles;
    let relative = Path::new("Writing/Texts/field-notes.md");

    assert!(!files.creation_target_exists(&root, relative).unwrap());
    let canonical = files.create_empty_text(&root, relative).unwrap();
    let expected = soyeht_platform::fs::canonicalize(&root.join(relative)).unwrap();
    assert_eq!(canonical, expected);
    assert_eq!(fs::read(&canonical).unwrap(), Vec::<u8>::new());
    assert!(files.creation_target_exists(&root, relative).unwrap());
    assert_eq!(files.create_empty_text(&root, relative).unwrap(), canonical);

    fs::write(&canonical, "existing work").unwrap();
    assert_eq!(
        files.create_empty_text(&root, relative).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    fs::create_dir(root.join("Writing/Screenplays")).unwrap();
    soyeht_platform::fs::symlink(&outside, root.join("Writing/Screenplays/linked")).unwrap();
    assert!(
        files
            .create_empty_text(
                &root,
                Path::new("Writing/Screenplays/linked/scene.fountain")
            )
            .is_err()
    );
    assert!(!outside.join("scene.fountain").exists());

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn binary_creation_publishes_exact_content_once_and_never_clobbers() {
    let root = std::env::temp_dir().join(format!("sn-binary-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let files = SystemFiles;
    let relative = Path::new("Writing/Presentations/assets/digest.png");
    let bytes = b"\x89PNG\r\n\x1a\nfirst";

    let canonical = files
        .create_binary_if_absent_or_matching(&root, relative, bytes)
        .unwrap();
    assert_eq!(fs::read(&canonical).unwrap(), bytes);
    assert_eq!(
        files
            .create_binary_if_absent_or_matching(&root, relative, bytes)
            .unwrap(),
        canonical
    );
    assert_eq!(
        files
            .create_binary_if_absent_or_matching(&root, relative, b"\x89PNG\r\n\x1a\ndifferent")
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(&canonical).unwrap(), bytes);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_file_that_cannot_be_text_is_refused_with_its_own_code() {
    use soyeht_protocol::ErrorKind;
    let root = std::env::temp_dir().join(format!("sn-refusal-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let binary = root.join("blob.bin");
    fs::write(&binary, [0xff, 0xfe, 0x00, 0x9f]).unwrap();
    let large = root.join("large.txt");
    fs::write(
        &large,
        vec![b'a'; soyeht_protocol::limits::EDITOR_HISTORY_BYTES + 1],
    )
    .unwrap();
    let service = Service::new(Box::new(SystemFiles));
    let kind = |path: &Path| match service.read_disk(path) {
        Ok(_) => None,
        Err(error) => Some(errors::wire(error).data.kind),
    };
    assert_eq!(kind(&binary), Some(ErrorKind::FileNotText));
    assert_eq!(kind(&large), Some(ErrorKind::FileTooLarge));
    assert_eq!(kind(&root), Some(ErrorKind::InvalidParams));
    fs::remove_dir_all(root).unwrap();
}
