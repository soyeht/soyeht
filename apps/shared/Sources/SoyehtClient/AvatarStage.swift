import CoreGraphics
import Foundation
import Observation
import SwiftUI

/// One avatar on a stage: its package loaded and drawn, the runtime stepped by the clock of the
/// screen. The views only draw `image`; the state of the conversation goes in through `state`.
/// The voice (the audio that plays, so the mouth speaks) is a later slice's input: until then the
/// mouth holds the expression of the state and the head lives.
@MainActor @Observable
public final class AvatarStageModel {
  /// What the avatar is doing: it listens, thinks, speaks or waits.
  public var state = AvatarState.idle
  /// The picture of the last frame.
  private(set) var image: CGImage?
  /// The size of the picture's pixels.
  private(set) var pictureSize = CGSize.zero

  /// The audio that plays now (the position in the clip and its level), when something does.
  var audio: AvatarAudio?

  private var runtime: AvatarRuntime
  private let renderer: AvatarRenderer
  private var lastTime: TimeInterval?

  /// Read the package in `folder` (an `avatar.json` and the PNG files it lists). A package that
  /// breaks a load rule throws, and nothing is drawn.
  public convenience init(folder: URL) throws {
    try self.init(
      loaded: try AvatarPackageFolder.load(folder), seed: UInt64.random(in: UInt64.min...UInt64.max)
    )
  }

  init(loaded: AvatarLoadedPackage, seed: UInt64, maxSide: Int = 512) throws {
    renderer = try loaded.renderer(maxSide: maxSide)
    runtime = AvatarRuntime(package: loaded.package, tune: AvatarTune(), seed: seed)
    pictureSize = CGSize(width: renderer.outputWidth, height: renderer.outputHeight)
    advance(to: 0)
  }

  /// Step the performance to `time` (seconds on any clock that only goes forward) and draw it.
  func advance(to time: TimeInterval) {
    let dt = lastTime.map { time - $0 } ?? 0
    lastTime = time
    let frame = runtime.step(dt: dt, state: state, audio: audio)
    image = renderer.render(frame).cgImage
  }

  /// Keep the stage alive until the task is cancelled: a frame every `interval` seconds.
  func run(every interval: Duration = .milliseconds(16)) async {
    let clock = ContinuousClock()
    let start = clock.now
    while !Task.isCancelled {
      let elapsed = clock.now - start
      advance(
        to: Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) * 1e-18)
      do { try await Task.sleep(for: interval) } catch { return }
    }
  }
}

/// The avatar's picture, drawn at the largest size that fits and kept in its proportions. It
/// carries no colour of its own: the view that hosts it gives the background.
public struct AvatarStageView: View {
  let model: AvatarStageModel

  public init(model: AvatarStageModel) { self.model = model }

  public var body: some View {
    Group {
      if let image = model.image {
        Image(decorative: image, scale: 1)
          .resizable()
          .interpolation(.medium)
          .aspectRatio(model.pictureSize, contentMode: .fit)
      } else {
        Color.clear
      }
    }
    .task { await model.run() }
  }
}
