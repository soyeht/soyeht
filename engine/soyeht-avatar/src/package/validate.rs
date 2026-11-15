//! The load rules of a package. Each failure names a kind (stable, in camelCase) and where in the
//! document it was found, never the value: the document is data from a person or an agent.

use std::collections::HashSet;

use super::acting_bounds;
use super::limits::{
    MAX_FILE_BYTES, MAX_FILES, MAX_NAME_BYTES, MAX_PATH_BYTES, MAX_PIXELS, MAX_SIDE,
    MAX_TEXTURE_POWER, MAX_TOTAL_BYTES, MAX_TRIANGLES, MAX_VERTICES,
};
use super::model::{EYE_IDS, FORMAT, MeshPart, Package, PartKind, Patch, Region, VERSION, Warp};
use crate::params::Shape;

/// What the caller knows about a file of the package folder: its size in bytes and, when it is a
/// PNG, its dimensions (`png_size`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    pub path: String,
    pub bytes: u64,
    pub size: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Not JSON, or not the shape of a package.
    Malformed,
    WrongFormat,
    UnsupportedVersion,
    /// The canvas is empty or too large.
    Canvas,
    /// A file path that is not a relative, plain `.png` path, is listed twice, or is named but
    /// not listed.
    File,
    /// A listed file is not among the assets the caller has.
    MissingAsset,
    /// An asset is not the size the package says (or is not a PNG).
    AssetSize,
    /// More files, bytes, vertices or triangles than a package may have.
    TooLarge,
    /// The shapes of a part are not the nine drawings, each once.
    Shapes,
    /// The nine shapes of a part do not share one vertex list, or the parts are not skin, lips and
    /// inner once each.
    Topology,
    /// A triangle names a vertex that does not exist.
    Index,
    /// A number is out of its range: a position far outside the canvas, a texture coordinate
    /// outside the shape's cell, an alpha outside 0..=1.
    Value,
    /// The eye frames are not the eight a package carries, or lie outside the canvas.
    Frames,
    /// The warp's places are outside the canvas or not shaped like places.
    Warp,
    /// An acting constant is out of its range.
    Acting,
    /// The atlas has fewer cells than shapes, or its picture is not the size of its cells.
    Atlas,
}

impl ErrorKind {
    /// The name used in the vector files and in logs.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Malformed => "malformed",
            ErrorKind::WrongFormat => "wrongFormat",
            ErrorKind::UnsupportedVersion => "unsupportedVersion",
            ErrorKind::Canvas => "canvas",
            ErrorKind::File => "file",
            ErrorKind::MissingAsset => "missingAsset",
            ErrorKind::AssetSize => "assetSize",
            ErrorKind::TooLarge => "tooLarge",
            ErrorKind::Shapes => "shapes",
            ErrorKind::Topology => "topology",
            ErrorKind::Index => "index",
            ErrorKind::Value => "value",
            ErrorKind::Frames => "frames",
            ErrorKind::Warp => "warp",
            ErrorKind::Acting => "acting",
            ErrorKind::Atlas => "atlas",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageError {
    pub kind: ErrorKind,
    /// A JSON pointer to the place in the document, such as `/mouth/parts/1/shapes/3/uvs`.
    pub at: String,
}

type Check = Result<(), PackageError>;

fn fail(kind: ErrorKind, at: impl Into<String>) -> Check {
    Err(PackageError {
        kind,
        at: at.into(),
    })
}

/// Slack for a texture coordinate at the edge of its cell.
const UV_SLACK: f64 = 1e-3;

pub(super) fn validate(p: &Package, assets: &[AssetInfo]) -> Check {
    if p.format != FORMAT {
        return fail(ErrorKind::WrongFormat, "/format");
    }
    // The format is unstable (0.x): the one version this crate reads, spelled exactly.
    if p.version != VERSION {
        return fail(ErrorKind::UnsupportedVersion, "/version");
    }
    let size = p.canvas;
    if size.width == 0 || size.height == 0 || size.width > MAX_SIDE || size.height > MAX_SIDE {
        return fail(ErrorKind::Canvas, "/canvas");
    }
    files(p, assets)?;
    mouth(p)?;
    frames(p, &p.eyes, &EYE_IDS, "/eyes")?;
    warp(p)?;
    match acting_bounds::first_out_of_range(&p.acting) {
        None => Ok(()),
        Some(at) => fail(ErrorKind::Acting, format!("/acting{at}")),
    }
}

/// Names Windows reserves, whatever the extension and the case.
fn reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| ('1'..='9').contains(&c)))
}

/// A path inside the package folder that is the same on every system: lower-case `.png`, names of
/// plain ASCII letters, digits, `.`, `_` and `-`, no empty, hidden or dot-only names, no name that
/// ends in a dot (Windows drops it), no reserved Windows name. That also refuses a control
/// character, a bidirectional override and a letter that only looks like another.
fn plain_png_path(path: &str) -> bool {
    let name_ok = |name: &str| {
        !name.is_empty()
            && name.len() <= MAX_NAME_BYTES
            && name.starts_with(|c: char| c.is_ascii_alphanumeric())
            && !name.ends_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            && !reserved_name(name)
    };
    path.len() <= MAX_PATH_BYTES && path.ends_with(".png") && path.split('/').all(name_ok)
}

fn files(p: &Package, assets: &[AssetInfo]) -> Check {
    if p.files.len() > MAX_FILES {
        return fail(ErrorKind::TooLarge, "/files");
    }
    // Two names that differ only in case are one file on some systems.
    let mut seen = HashSet::new();
    let (mut total, mut pixels) = (0u64, 0u64);
    for (i, f) in p.files.iter().enumerate() {
        let at = format!("/files/{i}");
        if !plain_png_path(&f.path) || !seen.insert(f.path.to_ascii_lowercase()) {
            return fail(ErrorKind::File, at);
        }
        if f.width == 0 || f.height == 0 || f.width > MAX_SIDE || f.height > MAX_SIDE {
            return fail(ErrorKind::AssetSize, at);
        }
        let Some(asset) = assets.iter().find(|a| a.path == f.path) else {
            return fail(ErrorKind::MissingAsset, at);
        };
        if asset.bytes > MAX_FILE_BYTES {
            return fail(ErrorKind::TooLarge, at);
        }
        if asset.size != Some((f.width, f.height)) {
            return fail(ErrorKind::AssetSize, at);
        }
        total += asset.bytes;
        pixels += u64::from(f.width) * u64::from(f.height);
    }
    if total > MAX_TOTAL_BYTES || pixels > MAX_PIXELS {
        return fail(ErrorKind::TooLarge, "/files");
    }
    let listed = |path: &str| p.files.iter().find(|f| f.path == path);
    // The base picture is the whole canvas.
    match listed(&p.base) {
        Some(base) if (base.width, base.height) == (p.canvas.width, p.canvas.height) => {}
        Some(_) => return fail(ErrorKind::Canvas, "/base"),
        None => return fail(ErrorKind::File, "/base"),
    }
    let named = std::iter::once(("/mouth/atlas/file".to_string(), &p.mouth.atlas.file)).chain(
        p.eyes
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("/eyes/{i}/file"), &f.file)),
    );
    for (at, path) in named {
        if listed(path).is_none() {
            return fail(ErrorKind::File, at);
        }
    }
    Ok(())
}

fn mouth(p: &Package) -> Check {
    let atlas = &p.mouth.atlas;
    let cells = u64::from(atlas.columns) * u64::from(atlas.rows);
    if atlas.columns == 0 || atlas.rows == 0 || cells < Shape::ALL.len() as u64 {
        return fail(ErrorKind::Atlas, "/mouth/atlas");
    }
    // The atlas picture is a whole number of cells, and the cells are the size of a mouth crop.
    let Some(file) = p.files.iter().find(|f| f.path == atlas.file) else {
        return fail(ErrorKind::File, "/mouth/atlas/file");
    };
    if !file.width.is_multiple_of(atlas.columns) || !file.height.is_multiple_of(atlas.rows) {
        return fail(ErrorKind::Atlas, "/mouth/atlas");
    }
    let kinds: Vec<PartKind> = p.mouth.parts.iter().map(|part| part.id).collect();
    if kinds.len() != PartKind::ALL.len() || PartKind::ALL.iter().any(|k| !kinds.contains(k)) {
        return fail(ErrorKind::Topology, "/mouth/parts");
    }
    for (i, part) in p.mouth.parts.iter().enumerate() {
        mesh_part(p, part, &format!("/mouth/parts/{i}"))?;
    }
    Ok(())
}

fn mesh_part(p: &Package, part: &MeshPart, at: &str) -> Check {
    let order: Vec<&str> = Shape::ALL.iter().map(|s| s.id()).collect();
    let ids: HashSet<&str> = part.shapes.iter().map(|s| s.id.as_str()).collect();
    if part.shapes.len() != order.len()
        || ids.len() != order.len()
        || order.iter().any(|id| !ids.contains(id))
    {
        return fail(ErrorKind::Shapes, format!("{at}/shapes"));
    }
    let Some(first) = part.shapes.first() else {
        return fail(ErrorKind::Shapes, format!("{at}/shapes"));
    };
    let floats = first.positions.len();
    if floats == 0 || !floats.is_multiple_of(2) || floats / 2 > MAX_VERTICES {
        return fail(
            if floats / 2 > MAX_VERTICES {
                ErrorKind::TooLarge
            } else {
                ErrorKind::Topology
            },
            format!("{at}/shapes/0/positions"),
        );
    }
    let vertices = floats / 2;
    if part.triangles.is_empty() || !part.triangles.len().is_multiple_of(3) {
        return fail(ErrorKind::Topology, format!("{at}/triangles"));
    }
    if part.triangles.len() / 3 > MAX_TRIANGLES {
        return fail(ErrorKind::TooLarge, format!("{at}/triangles"));
    }
    if part.triangles.iter().any(|t| *t as usize >= vertices) {
        return fail(ErrorKind::Index, format!("{at}/triangles"));
    }
    if let Some(alphas) = &part.alphas {
        if alphas.len() != vertices {
            return fail(ErrorKind::Topology, format!("{at}/alphas"));
        }
        if alphas.iter().any(|a| !(0.0..=1.0).contains(a)) {
            return fail(ErrorKind::Value, format!("{at}/alphas"));
        }
    }
    if !(1.0..=MAX_TEXTURE_POWER).contains(&part.texture_power) {
        return fail(ErrorKind::Value, format!("{at}/texturePower"));
    }
    let (w, h) = (f64::from(p.canvas.width), f64::from(p.canvas.height));
    let atlas = &p.mouth.atlas;
    let mut textured = false;
    for (i, shape) in part.shapes.iter().enumerate() {
        let here = format!("{at}/shapes/{i}");
        if shape.positions.len() != floats {
            return fail(ErrorKind::Topology, format!("{here}/positions"));
        }
        // Positions may leave the canvas a little (the feather), but never by more than its size.
        let outside = |v: &f64, side: f64| !v.is_finite() || *v < -side || *v > 2.0 * side;
        if shape
            .positions
            .chunks(2)
            .any(|xy| outside(&xy[0], w) || outside(&xy[1], h))
        {
            return fail(ErrorKind::Value, format!("{here}/positions"));
        }
        let Some(uvs) = &shape.uvs else { continue };
        textured = true;
        if uvs.len() != floats {
            return fail(ErrorKind::Topology, format!("{here}/uvs"));
        }
        // The shape's cell in the atlas, by its place in the order of the nine drawings.
        let cell = order.iter().position(|id| *id == shape.id).unwrap_or(0) as u32;
        let (col, row) = (cell % atlas.columns, cell / atlas.columns);
        let (cw, ch) = (1.0 / f64::from(atlas.columns), 1.0 / f64::from(atlas.rows));
        let (u0, v0) = (f64::from(col) * cw, f64::from(row) * ch);
        let bad = |uv: &[f64]| {
            !(u0 - UV_SLACK..=u0 + cw + UV_SLACK).contains(&uv[0])
                || !(v0 - UV_SLACK..=v0 + ch + UV_SLACK).contains(&uv[1])
        };
        if uvs.chunks(2).any(bad) {
            return fail(ErrorKind::Value, format!("{here}/uvs"));
        }
    }
    if textured {
        Ok(())
    } else {
        fail(ErrorKind::Value, format!("{at}/shapes"))
    }
}

fn frames(p: &Package, frames: &[Patch], ids: &[&str], at: &str) -> Check {
    let found: HashSet<&str> = frames.iter().map(|f| f.id.as_str()).collect();
    if frames.len() != ids.len()
        || found.len() != ids.len()
        || ids.iter().any(|id| !found.contains(id))
    {
        return fail(ErrorKind::Frames, at);
    }
    for (i, f) in frames.iter().enumerate() {
        let [x, y, w, h] = f.rect;
        let inside = w > 0
            && h > 0
            && u64::from(x) + u64::from(w) <= u64::from(p.canvas.width)
            && u64::from(y) + u64::from(h) <= u64::from(p.canvas.height);
        let picture = p.files.iter().find(|file| file.path == f.file);
        if !inside || picture.is_none_or(|file| (file.width, file.height) != (w, h)) {
            return fail(ErrorKind::Frames, format!("{at}/{i}"));
        }
    }
    Ok(())
}

fn region_ok(r: &Region, w: f64, h: f64) -> bool {
    let [cx, cy] = r.center;
    let [rx, ry] = r.radii;
    (0.0..=w).contains(&cx)
        && (0.0..=h).contains(&cy)
        && rx > 0.0
        && ry > 0.0
        && rx <= w
        && ry <= h
        && (0.0..1.0).contains(&r.inner)
}

fn warp(p: &Package) -> Check {
    let (w, h) = (f64::from(p.canvas.width), f64::from(p.canvas.height));
    let k: &Warp = &p.warp;
    let inside = |at: [f64; 2]| (0.0..=w).contains(&at[0]) && (0.0..=h).contains(&at[1]);
    let regions = [&k.head, &k.face, &k.brows[0], &k.brows[1], &k.jaw, &k.cheek];
    let fade = k.head_fade;
    let chest = &k.chest;
    // A spread of the chest over 5 % of the distance from the centre is not breathing, and a
    // rise over 2 % of the picture's height is not either (the lab's are 0.6 % and 0.35 %).
    // The left brow is left of the right one: the tilt of each runs from its outer to its inner end.
    let ok = inside(k.pivot)
        && k.brows[0].center[0] < k.brows[1].center[0]
        && inside(k.zoom_center)
        && regions.iter().all(|r| region_ok(r, w, h))
        && 0.0 <= fade[0]
        && fade[0] < fade[1]
        && fade[1] <= h
        && k.brow_tilt_span > 0.0
        && k.brow_tilt_span <= w
        && 0.0 <= chest.from
        && chest.from < chest.to
        && chest.to <= h
        && (0.0..=w).contains(&chest.center_x)
        && (0.0..=0.05).contains(&chest.spread)
        && (0.0..=0.02 * h).contains(&chest.rise);
    if ok {
        Ok(())
    } else {
        fail(ErrorKind::Warp, "/warp")
    }
}
