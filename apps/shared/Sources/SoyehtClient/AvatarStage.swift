import CoreGraphics
import Foundation
import Observation
import SwiftUI

/// Decides how large the picture is drawn. A frame that costs more than the time it has makes the
/// next ones smaller (by a quarter of the side, down to `minimumSide`); the side never grows
/// again, so a machine that cannot keep up does not oscillate. The cost is that of the output, so
/// a debug build, which runs the rasteriser unoptimised, settles on a small picture and a
/// release build keeps the 512 it asks for.
struct AvatarFramePacer: Equatable, Sendable {
  static let minimumSide = 96
  /// How many frames in a row must be too slow before the picture shrinks.
  static let patience = 6

  private(set) var side: Int
  private var slow = 0

  init(side: Int) { self.side = side }

  /// Record what a frame cost against the time it had, in seconds.
  mutating func record(cost: Double, allowed: Double) {
    guard side > Self.minimumSide else { return }
    slow = cost > allowed ? slow + 1 : 0
    if slow >= Self.patience {
      side = max(Self.minimumSide, side * 3 / 4)
      slow = 0
    }
  }
}

/// A finished picture, handed from the actor that drew it to the main actor that shows it.
struct AvatarStagePicture: @unchecked Sendable {
  /// A `CGImage` is immutable once made, and the actor keeps no reference to it.
  let image: CGImage
  let size: CGSize
}

/// Owns one avatar's runtime and renderer, so that the frames and the load run off the main
/// actor: the main actor only assigns the picture that comes back.
actor AvatarStageEngine {
  private var runtime: AvatarRuntime
  private var renderer: AvatarRenderer
  private let loaded: AvatarLoadedPackage
  private var pacer: AvatarFramePacer
  private let clock = ContinuousClock()

  init(loaded: AvatarLoadedPackage, seed: UInt64, maxSide: Int) throws {
    self.loaded = loaded
    renderer = try loaded.renderer(maxSide: maxSide)
    runtime = AvatarRuntime(package: loaded.package, tune: AvatarTune(), seed: seed)
    pacer = AvatarFramePacer(side: maxSide)
  }

  /// Read a package folder and make its engine, off the main actor.
  @concurrent
  static func load(folder: URL, seed: UInt64, maxSide: Int) async throws -> AvatarStageEngine {
    try AvatarStageEngine(
      loaded: try AvatarPackageFolder.load(folder), seed: seed, maxSide: maxSide)
  }

  /// Step by `dt` and draw. `allowed` is the time the frame has, in seconds: a frame that keeps
  /// costing more shrinks the picture.
  func frame(dt: Double, state: AvatarState, allowed: Double) -> AvatarStagePicture? {
    let started = clock.now
    let frame = runtime.step(dt: dt, state: state, audio: nil)
    let picture = draw(frame)
    pacer.record(cost: seconds(clock.now - started), allowed: allowed)
    if pacer.side < renderer.outputSide {
      do {
        renderer = try loaded.renderer(maxSide: pacer.side)
      } catch {
        // The same pictures drew the larger one, so this is not expected: keep it, and stop
        // asking for a smaller one.
        pacer = AvatarFramePacer(side: renderer.outputSide)
      }
    }
    return picture
  }

  /// The face of a state with no motion in it (Reduce Motion): the runtime is stepped to where it
  /// would be a second after the state began, and one picture is drawn.
  func stillFrame(state: AvatarState) -> AvatarStagePicture? {
    var frame = runtime.step(dt: 0.1, state: state, audio: nil)
    for _ in 1..<10 { frame = runtime.step(dt: 0.1, state: state, audio: nil) }
    return draw(frame)
  }

  private func draw(_ frame: AvatarFrame) -> AvatarStagePicture? {
    renderer.render(frame).cgImage.map {
      AvatarStagePicture(
        image: $0, size: CGSize(width: renderer.outputWidth, height: renderer.outputHeight))
    }
  }

  private func seconds(_ d: Duration) -> Double {
    Double(d.components.seconds) + Double(d.components.attoseconds) * 1e-18
  }
}

/// One avatar on a stage: its package loaded and drawn by an actor, the picture shown by the
/// views. The state of the conversation goes in through `state`. The voice (the audio that plays,
/// so the mouth speaks) is a later slice's input: until then the mouth holds the expression of the
/// state and the head lives.
///
/// Frames are paced by completion, not by a timer: the next frame starts when the last one has
/// been drawn and shown, no sooner than 1/30 s after it started (1/60 s while speaking). The loop
/// runs while at least one view of the model is on screen and its scene is active, and there is
/// one loop however many views show the model.
@MainActor @Observable
public final class AvatarStageModel {
  /// What the avatar is doing: it listens, thinks, speaks or waits.
  public var state = AvatarState.idle

  /// The picture of the last frame.
  private(set) var image: CGImage?
  /// The size of the picture's pixels.
  private(set) var pictureSize = CGSize.zero
  /// Why the package did not load, when it did not.
  private(set) var failure: AvatarPackageErrorKind?
  /// Reduce Motion is on: one still picture per state, no loop of frames.
  var reducesMotion = false

  private enum Source {
    case folder(URL)
    case loaded(AvatarLoadedPackage, seed: UInt64)
  }

  private let source: Source
  private let maxSide: Int
  private var engine: AvatarStageEngine?
  private var viewing = Set<UUID>()
  private var loop: Task<Void, Never>?

  /// A stage for the package in `folder` (an `avatar.json` and the PNG files it lists). It is read
  /// when the first view appears, off the main actor; a package that breaks a load rule leaves
  /// `failure` and draws nothing.
  public init(folder: URL) {
    source = .folder(folder)
    maxSide = 512
  }

  init(loaded: AvatarLoadedPackage, seed: UInt64, maxSide: Int = 512) {
    source = .loaded(loaded, seed: seed)
    self.maxSide = maxSide
  }

  /// A view says whether it shows the model now (on screen, in an active scene). The loop runs
  /// while any view does.
  func setViewing(_ viewer: UUID, _ showing: Bool) {
    if showing { viewing.insert(viewer) } else { viewing.remove(viewer) }
    if viewing.isEmpty {
      loop?.cancel()
      loop = nil
    } else if loop == nil {
      loop = Task { await self.run() }
    }
  }

  /// Whether the frame loop is running.
  var isRunning: Bool { loop != nil }

  private func makeEngine() async throws -> AvatarStageEngine {
    switch source {
    case .folder(let url):
      return try await AvatarStageEngine.load(
        folder: url, seed: UInt64.random(in: UInt64.min...UInt64.max), maxSide: maxSide)
    case .loaded(let package, let seed):
      return try AvatarStageEngine(loaded: package, seed: seed, maxSide: maxSide)
    }
  }

  private func run() async {
    if engine == nil {
      do {
        engine = try await makeEngine()
      } catch let error as AvatarPackageError {
        failure = error.kind
        return
      } catch {
        failure = .malformed
        return
      }
    }
    guard let engine else { return }
    let clock = ContinuousClock()
    var last = clock.now
    var stillState: AvatarState?
    while !Task.isCancelled {
      let started = clock.now
      let state = state
      let interval = state == .speaking ? 1.0 / 60 : 1.0 / 30
      let picture: AvatarStagePicture?
      if reducesMotion {
        if stillState != state || image == nil {
          stillState = state
          picture = await engine.stillFrame(state: state)
        } else {
          picture = nil
        }
      } else {
        stillState = nil
        picture = await engine.frame(
          dt: Self.seconds(started - last), state: state, allowed: interval)
      }
      last = started
      if let picture {
        image = picture.image
        pictureSize = picture.size
      }
      let wait = reducesMotion ? 0.25 : max(interval - Self.seconds(clock.now - started), 0)
      do { try await Task.sleep(for: .seconds(wait)) } catch { return }
    }
  }

  private static func seconds(_ d: Duration) -> Double {
    Double(d.components.seconds) + Double(d.components.attoseconds) * 1e-18
  }
}

/// The avatar's picture, drawn at the largest size that fits and kept in its proportions. It
/// carries no colour of its own: the view that hosts it gives the background. It runs the model's
/// frames while it is on screen in an active scene, and says what the avatar is doing to
/// VoiceOver.
public struct AvatarStageView: View {
  let model: AvatarStageModel
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @Environment(\.scenePhase) private var scenePhase
  @State private var viewer = UUID()

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
    .accessibilityElement(children: .ignore)
    .accessibilityLabel("Avatar, \(model.state.rawValue)")
    .onAppear {
      model.reducesMotion = reduceMotion
      model.setViewing(viewer, scenePhase == .active)
    }
    .onDisappear { model.setViewing(viewer, false) }
    .onChange(of: scenePhase) { _, phase in model.setViewing(viewer, phase == .active) }
    .onChange(of: reduceMotion) { _, reduced in model.reducesMotion = reduced }
  }
}
