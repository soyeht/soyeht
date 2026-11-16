//! An avatar ready to draw: a validated package and the pictures it names, decoded.
//!
//! The crate `soyeht-avatar` holds every rule (the format, the bounds, the mouth, the pose); this
//! reads a package folder the way its docs ask a caller to (the document, then the size of each
//! picture it lists, then the load rules), and only then decodes the pictures it draws from.

use super::pixmap::Pixmap;
use serde::Deserialize;
use soyeht_avatar::{
    AssetInfo, ErrorKind, Eye, FileDecl, MAX_DOCUMENT_BYTES, MAX_FILE_BYTES, MAX_FILES, Package,
    PackageError, eye_frame, png_size,
};
use soyeht_platform::log::Failure;
use std::{
    fs::File,
    io::{self, Cursor, Read},
    path::{Component, Path},
};

/// The document of a package folder.
const MANIFEST: &str = "avatar.json";
/// The most a decoder may allocate for one picture: a 2048 by 2048 picture is 16 MiB of texels.
const DECODE_BUDGET: u64 = 64 * 1024 * 1024;
/// The PNG signature and its header chunk: what `png_size` reads.
const PNG_HEADER: u64 = 24;

/// Why an avatar did not load.
#[derive(Debug)]
pub enum LoadError {
    /// A file of the folder could not be read.
    Io(io::ErrorKind),
    /// The document broke a load rule of the package format.
    Package(PackageError),
    /// A picture is not a PNG the decoder reads, or is not the size the package says.
    Picture,
}

impl Failure for LoadError {
    /// A label, never the text of an error: a message can carry a path.
    fn failure_kind(&self) -> String {
        match self {
            LoadError::Io(kind) => format!("io_{kind:?}"),
            LoadError::Package(error) => format!("package_{}", error.kind.as_str()),
            LoadError::Picture => "picture".to_owned(),
        }
    }
}

/// A patch of the base picture, in its place: an eye frame.
pub(super) struct EyePatch {
    pub(super) id: String,
    pub(super) rect: [u32; 4],
    pub(super) picture: Pixmap,
}

pub struct Avatar {
    package: Package,
    base: Pixmap,
    atlas: Pixmap,
    eyes: Vec<EyePatch>,
}

impl Avatar {
    /// An avatar from a package that passed `Package::load` and the pictures it lists, which
    /// `picture` gives by path. Every picture must be the size the package declares.
    pub fn from_pictures(
        package: Package,
        mut picture: impl FnMut(&str) -> Result<Pixmap, LoadError>,
    ) -> Result<Self, LoadError> {
        let mut named = |path: &str| {
            let pixmap = picture(path)?;
            let declared = package.files.iter().find(|f| f.path == path);
            match declared {
                Some(f) if (f.width, f.height) == (pixmap.width(), pixmap.height()) => Ok(pixmap),
                _ => Err(LoadError::Picture),
            }
        };
        let base = named(&package.base)?;
        let atlas = named(&package.mouth.atlas.file)?;
        let eyes = package
            .eyes
            .iter()
            .map(|patch| {
                Ok(EyePatch {
                    id: patch.id.clone(),
                    rect: patch.rect,
                    picture: named(&patch.file)?,
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        Ok(Self {
            package,
            base,
            atlas,
            eyes,
        })
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    pub(super) fn base(&self) -> &Pixmap {
        &self.base
    }

    pub(super) fn atlas(&self) -> &Pixmap {
        &self.atlas
    }

    /// The frame that draws what the eyes do, if the base picture's open eyes are not it.
    pub(super) fn eye_patch(&self, eye: Eye) -> Option<&EyePatch> {
        let id = eye_frame(eye)?;
        self.eyes.iter().find(|patch| patch.id == id)
    }
}

/// What the document says about the pictures it lists, before any of it is checked.
#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    files: Vec<FileDecl>,
}

/// Loads the package in `dir`. The document is checked against the files it lists before any
/// picture is decoded, and a path it names that leaves the folder is never opened.
pub fn load_folder(dir: &Path) -> Result<Avatar, LoadError> {
    let json = read_limited(&dir.join(MANIFEST), MAX_DOCUMENT_BYTES as u64)?;
    let json = String::from_utf8(json).map_err(|_| {
        LoadError::Package(PackageError {
            kind: ErrorKind::Malformed,
            at: String::new(),
        })
    })?;
    // A document that does not parse has no listing: the load rules then name what is wrong.
    let declared = serde_json::from_str::<Listing>(&json).map_or_else(|_| Vec::new(), |l| l.files);
    let assets: Vec<AssetInfo> = declared
        .iter()
        .take(MAX_FILES + 1)
        .filter_map(|file| asset_info(dir, &file.path))
        .collect();
    let package = Package::load(&json, &assets).map_err(LoadError::Package)?;
    Avatar::from_pictures(package, |path| {
        let relative = plain_relative(path).ok_or(LoadError::Picture)?;
        decode(&read_limited(&dir.join(relative), MAX_FILE_BYTES)?)
    })
}

/// A path of the package folder that stays inside it: relative, with plain names only.
fn plain_relative(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    let plain = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    (plain && path.components().next().is_some()).then_some(path)
}

/// The size in bytes and the dimensions of a listed picture. A file that cannot be opened or a
/// path that leaves the folder gets no entry, so the load rules report it as missing.
fn asset_info(dir: &Path, path: &str) -> Option<AssetInfo> {
    let relative = plain_relative(path)?;
    let Ok(file) = File::open(dir.join(relative)) else {
        return None;
    };
    let Ok(meta) = file.metadata() else {
        return None;
    };
    let mut header = Vec::new();
    // A header that cannot be read leaves the size unknown, which the load rules refuse.
    let read = file.take(PNG_HEADER).read_to_end(&mut header);
    let size = read.is_ok().then(|| png_size(&header)).flatten();
    Some(AssetInfo {
        path: path.to_owned(),
        bytes: meta.len(),
        size,
    })
}

/// The bytes of a file of at most `limit` bytes.
fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, LoadError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|error| LoadError::Io(error.kind()))?;
    if bytes.len() as u64 > limit {
        return Err(LoadError::Io(io::ErrorKind::InvalidData));
    }
    Ok(bytes)
}

/// A PNG as a premultiplied picture.
fn decode(bytes: &[u8]) -> Result<Pixmap, LoadError> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_BUDGET);
    reader.limits(limits);
    let rgba = reader
        .decode()
        .map_err(|_| LoadError::Picture)?
        .into_rgba8();
    Pixmap::from_straight_rgba(rgba.width(), rgba.height(), rgba.as_raw()).ok_or(LoadError::Picture)
}
