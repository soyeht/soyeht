import Foundation

/// A layer of the picture: one drawing, drawn with this opacity over the layers before it.
struct AvatarLayer: Equatable, Sendable {
  var shape: AvatarShape
  var alpha: Double
}

/// The geometry of the mouth: the lip contours of the nine drawings, blended.
///
/// Every drawing has the same vertex list, placed on its own drawn lips, so the geometry is the
/// weighted sum of the drawings' vertices and the lips pass through every half-open state with a
/// half-size aperture, never as two half-visible mouths. The picture is the aligned weighted
/// average of the drawings: each textured shape is drawn over the previous ones through its own
/// texture coordinates, onto that one blended geometry.
extension AvatarMeshPart {
  /// Weights below this take no part in the picture.
  private static let minLayerWeight = 1e-4

  private static func shapeIndex(_ id: String) -> Int? { AvatarShape.ids.firstIndex(of: id) }

  /// The vertex positions (`x, y` pairs) of this part at the morph `weights`
  /// (`AvatarMorphWeights.all`, summing to 1).
  func blend(_ weights: [Double]) -> [Double] {
    var out = [Double](repeating: 0, count: shapes.first?.positions.count ?? 0)
    for shape in shapes {
      guard let index = Self.shapeIndex(shape.id), weights[index] > 0 else { continue }
      let w = weights[index]
      for k in 0..<min(out.count, shape.positions.count) { out[k] += w * shape.positions[k] }
    }
    return out
  }

  /// The layers of the picture at the morph `weights`, in the order of the drawings. Each textured
  /// drawing's opacity is its weight over the cumulative weight of the layers so far, which makes
  /// the layers an exact weighted average. When no textured drawing takes part the first one is
  /// drawn alone, unless the part hides when empty (the inner mouth).
  func layers(_ weights: [Double]) -> [AvatarLayer] {
    let textured = shapes.compactMap { shape in
      shape.uvs == nil ? nil : Self.shapeIndex(shape.id)
    }.sorted()
    var cumulative = 0.0
    var layers: [AvatarLayer] = []
    for index in textured where weights[index] > Self.minLayerWeight {
      let w = pow(weights[index], texturePower)
      cumulative += w
      layers.append(AvatarLayer(shape: AvatarShape.allCases[index], alpha: w / cumulative))
    }
    if layers.isEmpty, !hideWhenEmpty, let first = textured.first {
      layers.append(AvatarLayer(shape: AvatarShape.allCases[first], alpha: 1))
    }
    return layers
  }
}
