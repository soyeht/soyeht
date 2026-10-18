import SwiftUI

/// Where the track and the arc of a usage ring sit inside the square the ring is given.
///
/// A stroke is centred on its path, so a circle stroked at the bounds spills half its line width
/// outside them, where a scroll view or a clip cuts it. The rule: the track and the arc share one
/// path, inset by half the widest line, so the widest stroke ends exactly on the bounds and the
/// arc sits inside the track.
public struct UsageRingGeometry: Equatable, Sendable {
  public let arcWidth: CGFloat
  public let trackWidth: CGFloat

  public init(arcWidth: CGFloat, trackWidth: CGFloat) {
    self.arcWidth = arcWidth
    self.trackWidth = trackWidth
  }

  /// How far the shared path is inset from the bounds.
  public var pathInset: CGFloat { max(arcWidth, trackWidth) / 2 }

  /// The radius of the shared path in a ring of `diameter`.
  public func pathRadius(diameter: CGFloat) -> CGFloat { diameter / 2 - pathInset }

  /// The radius the outside of a stroke of `width` reaches.
  public func outerRadius(width: CGFloat, diameter: CGFloat) -> CGFloat {
    pathRadius(diameter: diameter) + width / 2
  }

  /// The radius the inside of a stroke of `width` reaches.
  public func innerRadius(width: CGFloat, diameter: CGFloat) -> CGFloat {
    pathRadius(diameter: diameter) - width / 2
  }
}

/// The track and the band-coloured arc of a usage ring, drawn the same way on every device.
/// Both strokes lie inside the frame the view is given (`UsageRingGeometry`).
public struct UsageRingGauge: View {
  private let presentation: UsagePresentation
  private let geometry: UsageRingGeometry
  private let track: Color

  public init(
    _ presentation: UsagePresentation, geometry: UsageRingGeometry, track: Color
  ) {
    self.presentation = presentation
    self.geometry = geometry
    self.track = track
  }

  public var body: some View {
    ZStack {
      Circle()
        .inset(by: geometry.pathInset)
        .stroke(track, lineWidth: geometry.trackWidth)
      if let fraction = presentation.fraction, let band = presentation.band {
        Circle()
          .inset(by: geometry.pathInset)
          .trim(from: 0, to: fraction)
          .stroke(band.color, style: StrokeStyle(lineWidth: geometry.arcWidth, lineCap: .round))
          .rotationEffect(.degrees(-90))
          .opacity(presentation.emphasis.arcOpacity)
      }
    }
  }
}
