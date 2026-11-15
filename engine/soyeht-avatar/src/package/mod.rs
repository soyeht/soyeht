//! The avatar package: the format, the loader and the validator. See `model` for the format.
//!
//! The crate does no IO: the caller reads `avatar.json` and lists the PNG files of the folder
//! (their sizes in bytes, and their dimensions through `png_size`), and `Package::load` checks the
//! document against them.

mod model;
mod png;
mod validate;

pub use model::{
    Atlas, EYE_IDS, FORMAT, FileDecl, Lattice, MAJOR_VERSION, MeshPart, MouthModel, Package,
    PartKind, Patch, REST_MOUTH_IDS, ShapeMesh, Size,
};
pub use png::png_size;
pub use validate::{AssetInfo, ErrorKind, PackageError};

impl Package {
    /// Parse a package document and check every load rule against the files the caller has.
    pub fn load(json: &str, assets: &[AssetInfo]) -> Result<Package, PackageError> {
        let package: Package = serde_json::from_str(json).map_err(|_| PackageError {
            kind: ErrorKind::Malformed,
            at: String::new(),
        })?;
        validate::validate(&package, assets)?;
        Ok(package)
    }
}
