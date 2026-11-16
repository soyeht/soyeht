//! An avatar ready to draw: a validated package and the pictures it names, decoded.
//!
//! The crate `soyeht-avatar` holds every rule (the format, the bounds, the mouth, the pose); this
//! reads a package folder the way its docs ask a caller to (the document, then the size of each
//! picture it lists, then the load rules), and only then decodes the pictures it draws from.

use super::pixmap::Pixmap;
use serde::Deserialize;
use soyeht_avatar::{
    AssetInfo, ErrorKind, FileDecl, MAX_DOCUMENT_BYTES, MAX_FILE_BYTES, MAX_FILES, MAX_TOTAL_BYTES,
    Package, PackageError, plain_png_path, png_size,
};
use soyeht_platform::{fs::read_regular_under, log::Failure};
use std::{collections::HashMap, io, io::Cursor, path::Path};

/// The document of a package folder.
const MANIFEST: &str = "avatar.json";
/// The most a decoder may allocate for one picture: a 2048 by 2048 picture is 16 MiB of texels.
const DECODE_BUDGET: u64 = 64 * 1024 * 1024;

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

    /// The eye frames, with the rectangle each takes on the canvas.
    pub(super) fn eyes(&self) -> &[EyePatch] {
        &self.eyes
    }
}

/// What the document says about the pictures it lists, before any of it is checked.
#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    files: Vec<FileDecl>,
}

/// Loads the package in `dir`. The names the document lists are checked against the package
/// rule for a path (`plain_png_path`) before anything is opened, and every file is opened once, without
/// following a link, only if it is a regular file (`soyeht_platform::fs::read_regular_under`): the
/// bytes that were measured for the load rules are the bytes that are decoded. The document is
/// checked against the files before any picture is decoded.
pub fn load_folder(dir: &Path) -> Result<Avatar, LoadError> {
    let json = read_regular_under(dir, Path::new(MANIFEST), MAX_DOCUMENT_BYTES as u64)
        .map_err(|error| LoadError::Io(error.kind()))?;
    let json = String::from_utf8(json).map_err(|_| {
        LoadError::Package(PackageError {
            kind: ErrorKind::Malformed,
            at: String::new(),
        })
    })?;
    // A document that does not parse has no listing: the load rules then name what is wrong.
    let declared = serde_json::from_str::<Listing>(&json).map_or_else(|_| Vec::new(), |l| l.files);
    let (assets, mut files) = read_listed(dir, &declared);
    let package = Package::load(&json, &assets).map_err(LoadError::Package)?;
    Avatar::from_pictures(package, |path| {
        decode(&files.remove(path).ok_or(LoadError::Picture)?)
    })
}

/// The pictures a document lists, read once each: what the load rules need to know of them, and
/// their bytes. A name that is not a plain path is not opened, and neither is one past the limits
/// of the load rules (it is told to the rules as larger than they allow, which refuses it); a file
/// that cannot be read at all is told to the rules as missing.
fn read_listed(dir: &Path, declared: &[FileDecl]) -> (Vec<AssetInfo>, HashMap<String, Vec<u8>>) {
    let (mut assets, mut files) = (Vec::new(), HashMap::new());
    let mut total = 0u64;
    for file in declared.iter().take(MAX_FILES + 1) {
        if !plain_png_path(&file.path) {
            continue;
        }
        let too_large = AssetInfo {
            path: file.path.clone(),
            bytes: MAX_FILE_BYTES + 1,
            size: None,
        };
        if total > MAX_TOTAL_BYTES {
            assets.push(too_large);
            continue;
        }
        match read_regular_under(dir, Path::new(&file.path), MAX_FILE_BYTES) {
            Ok(bytes) => {
                total += bytes.len() as u64;
                assets.push(AssetInfo {
                    path: file.path.clone(),
                    bytes: bytes.len() as u64,
                    size: png_size(&bytes),
                });
                files.insert(file.path.clone(), bytes);
            }
            // Larger than a picture may be, or not a regular file at all.
            Err(error) if error.kind() == io::ErrorKind::InvalidData => assets.push(too_large),
            // Missing, unreadable or behind a link: the rules say it is not there.
            Err(error) => soyeht_platform::log::failure("avatar.file_unreadable", &error),
        }
    }
    (assets, files)
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
