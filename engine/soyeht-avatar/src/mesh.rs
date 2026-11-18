//! The geometry of the mouth: the lip contours of the nine drawings, blended.
//!
//! Every drawing has the same vertex list, placed on its own drawn lips, so the geometry is the
//! weighted sum of the drawings' vertices and the lips pass through every half-open state with a
//! half-size aperture, never as two half-visible mouths. The picture is the aligned weighted
//! average of the drawings: each textured shape is drawn over the previous ones through its own
//! texture coordinates, onto that one blended geometry. Ported from `evalBlend` of
//! `rig/runtime/core.js` (Apache-2.0, the same authors).

use crate::package::{MeshPart, MouthModel, PartKind};
use crate::params::{SHAPES, Shape};

/// A layer of the picture: one drawing, drawn with this opacity over the layers before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub shape: Shape,
    pub alpha: f64,
}

/// Weights below this take no part in the picture.
const MIN_LAYER_WEIGHT: f64 = 1e-4;

impl MeshPart {
    fn shape_index(id: &str) -> Option<usize> {
        Shape::ALL.iter().position(|s| s.id() == id)
    }

    /// The vertex positions (`x, y` pairs) of this part at the morph `weights` (`MorphWeights::all`,
    /// summing to 1).
    pub fn blend(&self, weights: &[f64; SHAPES]) -> Vec<f64> {
        let len = self.shapes.first().map_or(0, |s| s.positions.len());
        let mut out = vec![0.0; len];
        for shape in &self.shapes {
            let Some(w) = Self::shape_index(&shape.id)
                .map(|i| weights[i])
                .filter(|w| *w > 0.0)
            else {
                continue;
            };
            for (o, v) in out.iter_mut().zip(&shape.positions) {
                *o += w * v;
            }
        }
        out
    }

    /// The layers of the picture at the morph `weights`, in the order of the drawings. Each
    /// textured drawing's opacity is its weight over the cumulative weight of the layers so far,
    /// which makes the layers an exact weighted average. When no textured drawing takes part the
    /// first one is drawn alone, unless the part hides when empty (the inner mouth). A part that
    /// is always drawn has a picture for every drawing (the loader refuses one that has not), so
    /// some drawing always takes part and none replaces another in one frame.
    pub fn layers(&self, weights: &[f64; SHAPES]) -> Vec<Layer> {
        let mut textured: Vec<(usize, bool)> = self
            .shapes
            .iter()
            .filter_map(|s| Some((Self::shape_index(&s.id)?, s.uvs.is_some())))
            .filter(|(_, textured)| *textured)
            .collect();
        textured.sort_by_key(|(i, _)| *i);
        let mut cumulative = 0.0;
        let mut layers = Vec::new();
        for (i, _) in &textured {
            if weights[*i] <= MIN_LAYER_WEIGHT {
                continue;
            }
            let w = weights[*i].powf(self.texture_power);
            cumulative += w;
            layers.push(Layer {
                shape: Shape::ALL[*i],
                alpha: w / cumulative,
            });
        }
        if let (true, Some((i, _))) = (layers.is_empty() && !self.hide_when_empty, textured.first())
        {
            layers.push(Layer {
                shape: Shape::ALL[*i],
                alpha: 1.0,
            });
        }
        layers
    }
}

impl MeshPart {
    /// How much of this part is drawn over what is below it, 0 to 1, before the feather of its
    /// corners. A part that is always drawn is drawn whole. A part that hides when empty (the
    /// inner mouth, which has no picture at rest, and which does cover an area there) is drawn as
    /// much as the drawings that have a picture of it take part: the sum of their weights, so it
    /// comes in as the mouth opens and not whole when a weight passes the threshold under which
    /// a drawing takes no part (`layers`). The coverage of `composite` is this times the feather.
    pub fn opacity(&self, weights: &[f64; SHAPES]) -> f64 {
        if !self.hide_when_empty {
            return 1.0;
        }
        let taking_part: f64 = Shape::ALL
            .iter()
            .zip(weights)
            .filter(|(shape, _)| {
                self.shapes
                    .iter()
                    .any(|m| m.id == shape.id() && m.uvs.is_some())
            })
            .map(|(_, w)| w)
            .sum();
        taking_part.clamp(0.0, 1.0)
    }
}

/// A part of the mouth laid over the picture, once. `layers` are the texel each drawing's
/// picture gives at this pixel with the `Layer::alpha` of the drawing, `under` is what is below
/// the part and `coverage` is the opacity of the part at this pixel (the feather of its corners
/// times `MeshPart::opacity`), 0 to 1. Every colour is premultiplied RGBA of 0 to 255.
///
/// The layers are first **mixed by their weights**, each over the mix before it
/// (`mix = texel * a + mix * (1 - a)`, `a` the layer's alpha, the first being 1): the texel's own
/// opacity is part of the colour that is mixed, so a clear texel of a heavy drawing makes the mix
/// clearer in proportion to its weight and a drawing coming in at a tiny weight changes the mix by
/// a tiny amount, whatever the texels' opacity. Then the mix is laid over `under` once, by
/// `coverage` (`mix * c + under * (1 - c * mix alpha)`). The feather counts once, however many
/// drawings are in play: laying each layer over the picture with the feather would count it once
/// for each drawing, and the edge of the part would step when a layer came in.
pub fn composite(under: [f64; 4], layers: &[([f64; 4], f64)], coverage: f64) -> [f64; 4] {
    let mut mix = [0.0; 4];
    for (texel, alpha) in layers {
        let a = alpha.clamp(0.0, 1.0);
        mix = [0, 1, 2, 3].map(|c| texel[c] * a + mix[c] * (1.0 - a));
    }
    let c = coverage.clamp(0.0, 1.0);
    let keep = 1.0 - c * mix[3] / 255.0;
    [0, 1, 2, 3].map(|k| mix[k] * c + under[k] * keep)
}

impl MouthModel {
    /// The parts in the order they are drawn, each over the one before: the skin around the
    /// lips, the ring of the lips, and last the inner mouth. A package lists its parts in any
    /// order (`drawOrder` of `contracts/vectors/avatar-package.json`); a part it lacks is
    /// skipped.
    pub fn in_drawing_order(&self) -> Vec<&MeshPart> {
        PartKind::ALL
            .iter()
            .filter_map(|kind| self.parts.iter().find(|part| part.id == *kind))
            .collect()
    }
}
