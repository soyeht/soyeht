//! The avatar package: the format, the loader and the validator. See `model` for the format.
//!
//! The crate does no IO: the caller reads `avatar.json` and lists the PNG files of the folder
//! (their sizes in bytes, and their dimensions through `png_size`), and `Package::load` checks the
//! document against them.

pub(crate) mod acting_bounds;
pub(crate) mod limits;
mod model;
mod png;
mod validate;

pub use limits::{MAX_DOCUMENT_BYTES, MAX_FILE_BYTES, MAX_FILES};
pub use model::{
    Atlas, Chest, EYE_IDS, FORMAT, FileDecl, MeshPart, MouthModel, Package, PartKind, Patch,
    Region, ShapeMesh, Size, VERSION, Warp,
};
pub use png::png_size;
pub use validate::{AssetInfo, ErrorKind, PackageError};

impl Package {
    /// Parse a package document and check every load rule against the files the caller has.
    pub fn load(json: &str, assets: &[AssetInfo]) -> Result<Package, PackageError> {
        // Before anything is parsed: a document from a stranger is not read at any length.
        if json.len() > limits::MAX_DOCUMENT_BYTES {
            return Err(PackageError {
                kind: ErrorKind::TooLarge,
                at: String::new(),
            });
        }
        let package: Package = serde_json::from_str(json).map_err(|_| PackageError {
            kind: ErrorKind::Malformed,
            at: String::new(),
        })?;
        validate::validate(&package, assets)?;
        Ok(package)
    }
}
