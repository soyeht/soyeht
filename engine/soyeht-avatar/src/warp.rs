//! Applying a `Pose` to a face: where in the picture each point of the picture drawn is read
//! from. The package says where the neck, the brows, the jaw and the other places of this face
//! are (`Warp`); this is the one rule that moves them, so the renderers (a shader, a software
//! rasteriser) agree. Ported from the fragment shader of `static/sabio-warp.js` (Apache-2.0, the
//! same authors), with the places read from the package in place of the numbers for one face.

use crate::acting::Pose;
use crate::package::{Region, Warp};

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The influence of a region at `p`: 1 inside the inner ellipse, falling to 0 at the radii.
fn influence(p: [f64; 2], r: &Region) -> f64 {
    let (dx, dy) = (
        (p[0] - r.center[0]) / r.radii[0],
        (p[1] - r.center[1]) / r.radii[1],
    );
    let squared = dx * dx + dy * dy;
    // Outside the ellipse the smoothstep is 1: no influence, and most points of a face are
    // outside most places, so the square root is spared for them.
    if squared >= 1.0 {
        return 0.0;
    }
    1.0 - smoothstep(r.inner, 1.0, squared.sqrt())
}

impl Warp {
    /// Where the picture is read from to draw the point `(x, y)` of the canvas, with this pose:
    /// the point displaced by the head, the face, the brows, the jaw, the cheeks and the chest,
    /// and the zoom. The point it returns can lie outside the canvas (near an edge, with the head
    /// turned or the zoom in): the reader clamps it to the picture when it samples, as a texture
    /// does with clamp-to-edge.
    pub fn source_point(&self, pose: &Pose, x: f64, y: f64) -> [f64; 2] {
        let zoom = pose.zoom.max(1e-6);
        let p = [
            (x - self.zoom_center[0]) / zoom + self.zoom_center[0],
            (y - self.zoom_center[1]) / zoom + self.zoom_center[1],
        ];
        // The head rotates about the neck; its influence fades over the neck, so the collar stays.
        let head = influence(p, &self.head)
            * (1.0 - smoothstep(self.head_fade[0], self.head_fade[1], p[1]));
        let (hx, hy) = (p[0] - self.pivot[0], p[1] - self.pivot[1]);
        let (sin, cos) = if pose.rot == 0.0 {
            (0.0, 1.0)
        } else {
            pose.rot.to_radians().sin_cos()
        };
        let rotated = [cos * hx - sin * hy, sin * hx + cos * hy];
        let mut d = [
            head * (rotated[0] - hx + pose.head_x),
            head * (rotated[1] - hy + pose.head_y),
        ];
        // The face features move a little more than the outline: a small turn.
        let face = influence(p, &self.face);
        d[0] += face * pose.face_x;
        d[1] += face * pose.face_y;
        // The brows: a soft blob each, the inner end lifting more than the outer one.
        let [left, right] = &self.brows;
        let tilt_left =
            ((p[0] - left.center[0]) / self.brow_tilt_span).clamp(-1.0, 1.0) * 0.5 + 0.5;
        let tilt_right =
            ((right.center[0] - p[0]) / self.brow_tilt_span).clamp(-1.0, 1.0) * 0.5 + 0.5;
        d[1] -= influence(p, left) * (pose.brow_l + pose.brow_tilt * (tilt_left - 0.5));
        d[1] -= influence(p, right) * (pose.brow_r + pose.brow_tilt * (tilt_right - 0.5));
        // The lower face drops with the jaw, and the cheeks lift with a smile.
        let jaw = influence(p, &self.jaw);
        d[1] += jaw * pose.jaw;
        d[1] -= influence(p, &self.cheek) * (1.0 - jaw * 0.6) * pose.cheek;
        // The chest and shoulders breathe.
        let chest = &self.chest;
        let breath = smoothstep(chest.from, chest.to, p[1]) * pose.breath;
        d[0] += breath * (p[0] - chest.center_x) * chest.spread;
        d[1] -= breath * chest.rise;
        [p[0] - d[0], p[1] - d[1]]
    }
}
