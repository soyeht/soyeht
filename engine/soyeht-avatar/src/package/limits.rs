//! The limits of a package, in one place, each with the reason it has the value it has. A package
//! is a file from a stranger, so every size, count and range a reader allocates or loops by is
//! bounded before it is used. The vector file `avatar-package.json` pins each of them with a case
//! at the limit (loads) and one past it (fails).

/// The document itself, before it is parsed. The largest legal package (three parts of
/// `MAX_VERTICES` vertices, nine shapes, positions and texture coordinates, about ten bytes a
/// number) is about three megabytes; the lab's real model is seventy kilobytes.
pub(crate) const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
/// The largest side of the canvas and of any listed picture (pixels). A face picture a renderer
/// draws on a screen is not larger than a screen.
pub(crate) const MAX_SIDE: u32 = 2048;
/// Listed pictures: a base, an atlas, eight eye frames and a few more.
pub(crate) const MAX_FILES: usize = 64;
/// One picture file, and all of them together (bytes).
pub(crate) const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
/// The pictures once decoded, in pixels (four bytes each when decoded: 32 megabytes). A few
/// kilobytes of PNG can hold a 2048 by 2048 picture, so the file bytes alone do not bound this.
pub(crate) const MAX_PIXELS: u64 = 8 * 1024 * 1024;
/// A path of the package folder, and one name of it (bytes). Windows reads a path of 260.
pub(crate) const MAX_PATH_BYTES: usize = 128;
pub(crate) const MAX_NAME_BYTES: usize = 64;
/// Vertices of one part of the mouth mesh, and its triangles. The lab's mesh has a few hundred.
pub(crate) const MAX_VERTICES: usize = 3000;
pub(crate) const MAX_TRIANGLES: usize = 6000;
/// The power that sharpens the picture's weights.
pub(crate) const MAX_TEXTURE_POWER: f64 = 8.0;
