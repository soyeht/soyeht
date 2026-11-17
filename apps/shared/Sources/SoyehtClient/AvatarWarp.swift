import Foundation

/// Applying a pose to a face: where in the picture each point of the picture drawn is read from.
/// The package says where the neck, the brows, the jaw and the other places of this face are
/// (`AvatarWarp`); this is the one rule that moves them, so the renderers (this one, the desktop's
/// and the shaders of the lab) agree.
private func smoothstep(_ edge0: Double, _ edge1: Double, _ x: Double) -> Double {
  let t = min(max((x - edge0) / (edge1 - edge0), 0), 1)
  return t * t * (3.0 - 2.0 * t)
}

/// The influence of a region at `p`: 1 inside the inner ellipse, falling to 0 at the radii.
private func influence(_ p: (Double, Double), _ r: AvatarRegion) -> Double {
  let dx = (p.0 - r.center[0]) / r.radii[0]
  let dy = (p.1 - r.center[1]) / r.radii[1]
  return 1.0 - smoothstep(r.inner, 1.0, hypot(dx, dy))
}

extension AvatarWarp {
  /// Where the picture is read from to draw the point `(x, y)` of the canvas, with this pose:
  /// the point displaced by the head, the face, the brows, the jaw, the cheeks and the chest, and
  /// the zoom. The point it returns can lie outside the canvas (near an edge, with the head turned
  /// or the zoom in): the reader clamps it to the picture when it samples.
  func sourcePoint(pose: AvatarPose, x: Double, y: Double) -> (x: Double, y: Double) {
    let zoom = avatarMax(pose.zoom, 1e-6)
    let p = (
      (x - zoomCenter[0]) / zoom + zoomCenter[0], (y - zoomCenter[1]) / zoom + zoomCenter[1]
    )
    // The head rotates about the neck; its influence fades over the neck, so the collar stays.
    let head = influence(p, self.head) * (1.0 - smoothstep(headFade[0], headFade[1], p.1))
    let (hx, hy) = (p.0 - pivot[0], p.1 - pivot[1])
    let radians = pose.rot * (Double.pi / 180.0)
    let (sin, cos) = (Foundation.sin(radians), Foundation.cos(radians))
    let rotated = (cos * hx - sin * hy, sin * hx + cos * hy)
    var d = (
      head * (rotated.0 - hx + pose.headX), head * (rotated.1 - hy + pose.headY)
    )
    // The face features move a little more than the outline: a small turn.
    let faceInfluence = influence(p, face)
    d.0 += faceInfluence * pose.faceX
    d.1 += faceInfluence * pose.faceY
    // The brows: a soft blob each, the inner end lifting more than the outer one.
    let (left, right) = (brows[0], brows[1])
    let tiltLeft = min(max((p.0 - left.center[0]) / browTiltSpan, -1), 1) * 0.5 + 0.5
    let tiltRight = min(max((right.center[0] - p.0) / browTiltSpan, -1), 1) * 0.5 + 0.5
    d.1 -= influence(p, left) * (pose.browL + pose.browTilt * (tiltLeft - 0.5))
    d.1 -= influence(p, right) * (pose.browR + pose.browTilt * (tiltRight - 0.5))
    // The lower face drops with the jaw, and the cheeks lift with a smile.
    let jawInfluence = influence(p, jaw)
    d.1 += jawInfluence * pose.jaw
    d.1 -= influence(p, cheek) * (1.0 - jawInfluence * 0.6) * pose.cheek
    // The chest and shoulders breathe.
    let breath = smoothstep(chest.from, chest.to, p.1) * pose.breath
    d.0 += breath * (p.0 - chest.centerX) * chest.spread
    d.1 -= breath * chest.rise
    return (p.0 - d.0, p.1 - d.1)
  }
}
