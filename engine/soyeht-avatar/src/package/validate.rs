//! The load rules of a package. Each failure names a kind (stable, in camelCase) and where in the
//! document it was found, never the value: the document is data from a person or an agent.

use std::collections::HashSet;

use super::model::{
    EYE_IDS, FORMAT, MAJOR_VERSION, MeshPart, Package, PartKind, Patch, REST_MOUTH_IDS,
};
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
    /// The eye frames or the resting mouths are not the ones a package carries, or lie outside the
    /// canvas.
    Frames,
    /// The warp lattice is empty, too fine or outside the canvas.
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

/// The largest side of the canvas and of any listed picture (pixels).
const MAX_SIDE: u32 = 2048;
const MAX_FILES: usize = 64;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_VERTICES: usize = 3000;
const MAX_TRIANGLES: usize = 6000;
const MAX_LATTICE: u32 = 32;
/// Slack for a texture coordinate at the edge of its cell.
const UV_SLACK: f64 = 1e-3;

pub(super) fn validate(p: &Package, assets: &[AssetInfo]) -> Check {
    if p.format != FORMAT {
        return fail(ErrorKind::WrongFormat, "/format");
    }
    let major = p
        .version
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok());
    if major != Some(MAJOR_VERSION) || p.version.split('.').count() != 2 {
        return fail(ErrorKind::UnsupportedVersion, "/version");
    }
    let size = p.canvas;
    if size.width == 0 || size.height == 0 || size.width > MAX_SIDE || size.height > MAX_SIDE {
        return fail(ErrorKind::Canvas, "/canvas");
    }
    files(p, assets)?;
    mouth(p)?;
    frames(p)?;
    lattice(p)?;
    acting(p)
}

fn plain_png_path(path: &str) -> bool {
    path.ends_with(".png")
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}

fn files(p: &Package, assets: &[AssetInfo]) -> Check {
    if p.files.len() > MAX_FILES {
        return fail(ErrorKind::TooLarge, "/files");
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for (i, f) in p.files.iter().enumerate() {
        let at = format!("/files/{i}");
        if !plain_png_path(&f.path) || !seen.insert(f.path.as_str()) {
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
    }
    if total > MAX_TOTAL_BYTES {
        return fail(ErrorKind::TooLarge, "/files");
    }
    let listed = |path: &str| p.files.iter().any(|f| f.path == path);
    let named = std::iter::once(("/base".to_string(), &p.base))
        .chain(std::iter::once((
            "/mouth/atlas/file".to_string(),
            &p.mouth.atlas.file,
        )))
        .chain(
            p.eyes
                .iter()
                .enumerate()
                .map(|(i, f)| (format!("/eyes/{i}/file"), &f.file)),
        )
        .chain(
            p.rest_mouths
                .iter()
                .enumerate()
                .map(|(i, f)| (format!("/restMouths/{i}/file"), &f.file)),
        );
    for (at, path) in named {
        if !listed(path) {
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
    if !(1.0..=8.0).contains(&part.texture_power) {
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

fn frames(p: &Package) -> Check {
    check_frames(p, &p.eyes, &EYE_IDS, "/eyes")?;
    check_frames(p, &p.rest_mouths, &REST_MOUTH_IDS, "/restMouths")
}

fn check_frames(p: &Package, frames: &[Patch], ids: &[&str], at: &str) -> Check {
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

fn lattice(p: &Package) -> Check {
    let (l, [x, y, w, h]) = (&p.warp, p.warp.rect);
    let fits = w > 0
        && h > 0
        && u64::from(x) + u64::from(w) <= u64::from(p.canvas.width)
        && u64::from(y) + u64::from(h) <= u64::from(p.canvas.height);
    if !fits || !(1..=MAX_LATTICE).contains(&l.cols) || !(1..=MAX_LATTICE).contains(&l.rows) {
        return fail(ErrorKind::Warp, "/warp");
    }
    Ok(())
}

fn acting(p: &Package) -> Check {
    let a = &p.acting;
    let ordered = |r: [f64; 2]| r[0] > 0.0 && r[0] <= r[1] && r[1] <= 600.0;
    let chance = |c: f64| (0.0..=1.0).contains(&c);
    let ok = ordered(a.blink.gap)
        && ordered(a.blink.gap_listening)
        && ordered(a.gaze.idle_gap)
        && ordered(a.gaze.hold_away)
        && ordered(a.gaze.thinking_hold)
        && chance(a.blink.double_chance)
        && chance(a.blink.on_gaze_shift)
        && chance(a.gaze.speaking_away_chance)
        && chance(a.stress.chance)
        && a.blink.seq.iter().all(|s| *s > 0.0 && *s <= 5.0)
        && a.breath.period > 0.5
        && a.breath.period_speaking > 0.5
        && a.stress.min_gap >= 0.0
        && a.stress.word_gap >= 0.0
        && a.stress.db_over.is_finite();
    if ok {
        Ok(())
    } else {
        fail(ErrorKind::Acting, "/acting")
    }
}
