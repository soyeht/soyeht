import CoreGraphics
import Foundation
import Observation
import SoyehtAvatarRaster
import SwiftUI

/// Decides how large the picture is drawn, from what a frame costs against the time it has.
///
/// Frames that cost more than they have, `patience` in a row, make the picture a quarter smaller
/// (down to `minimumSide`). Frames that cost under half of it for a stretch make it a third larger
/// again (up to where it started), after `wait` seconds, which begins at ten and doubles every time
/// a regrown picture has to shrink again within it. The cost is that of the computing, not of the
/// wall clock (the engine measures the thread's CPU time), so a machine that is merely busy does
/// not shrink the picture of a build that is fast.
struct AvatarFramePacer: Equatable, Sendable {
  enum Change: Equatable, Sendable {
    case shrank(to: Int)
    case grew(to: Int)
  }

  static let minimumSide = 96
  /// How many frames in a row must be too slow before the picture shrinks.
  static let patience = 6
  static let firstWait = 10.0
  /// "Quiet" is judged against the shortest time a frame ever has (1/60 s, while speaking),
  /// whatever the state now: a picture that fits an idle frame (1/30 s) but not a speaking one
  /// would otherwise grow while idle and shrink at the next sentence, and pulse.
  static let quietAllowance = 1.0 / 60

  /// The largest the picture gets: the side it started at.
  let ceiling: Int
  private(set) var side: Int
  private var slow = 0
  private var quietSince: Double?
  private var wait = firstWait
  private var grewAt: Double?

  /// A pacer for a picture that starts at `side` pixels.
  init(side: Int) {
    self.side = side
    ceiling = side
  }

  /// Record what a frame cost against the time it had (seconds), at `now` (seconds on any clock
  /// that only goes forward). A change of the side is returned, to be made.
  mutating func record(cost: Double, allowed: Double, now: Double) -> Change? {
    if cost > allowed {
      slow += 1
      quietSince = nil
    } else {
      slow = 0
      quietSince = cost < Self.quietAllowance / 2 ? (quietSince ?? now) : nil
    }
    if slow >= Self.patience, side > Self.minimumSide {
      slow = 0
      // A regrown picture that cannot be kept is waited on longer the next time.
      if let grewAt, now - grewAt < wait { wait *= 2 }
      grewAt = nil
      side = max(Self.minimumSide, side * 3 / 4)
      return .shrank(to: side)
    }
    if let quietSince, now - quietSince >= wait, side < ceiling {
      self.quietSince = nil
      grewAt = now
      side = min(ceiling, (side * 4 + 2) / 3)
      return .grew(to: side)
    }
    return nil
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
  /// The pictures at full size, kept to draw the avatar again at another size.
  private let loaded: AvatarLoadedPackage
  private var pacer: AvatarFramePacer
  private let clock = ContinuousClock()
  private let started: ContinuousClock.Instant

  init(loaded: AvatarLoadedPackage, seed: UInt64, maxSide: Int) throws {
    self.loaded = loaded
    renderer = try loaded.renderer(maxSide: maxSide)
    runtime = AvatarRuntime(package: loaded.package, tune: AvatarTune(), seed: seed)
    pacer = AvatarFramePacer(side: renderer.outputSide)
    started = clock.now
  }

  /// Read a package folder and make its engine, off the main actor.
  @concurrent
  static func load(folder: URL, seed: UInt64, maxSide: Int) async throws -> AvatarStageEngine {
    try AvatarStageEngine(
      loaded: try AvatarPackageFolder.load(folder), seed: seed, maxSide: maxSide)
  }

  /// The time this thread has spent computing, in seconds. The methods of an actor are
  /// synchronous, so a frame runs on one thread from start to end.
  private static func cpuSeconds() -> Double {
    Double(clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)) / 1e9
  }

  /// Step by `dt` and draw. `allowed` is the time the frame has, in seconds: frames that keep
  /// costing more shrink the picture, and frames that cost little for long make it larger again.
  func frame(dt: Double, state: AvatarState, allowed: Double) -> AvatarStagePicture? {
    let before = Self.cpuSeconds()
    let frame = runtime.step(dt: dt, state: state, audio: nil)
    let picture = draw(frame)
    let cost = Self.cpuSeconds() - before
    let elapsed = clock.now - started
    let now = Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) * 1e-18
    if let change = pacer.record(cost: cost, allowed: allowed, now: now) {
      resize(for: change)
    }
    return picture
  }

  private func resize(for change: AvatarFramePacer.Change) {
    let (side, what): (Int, String)
    switch change {
    case .shrank(let to): (side, what) = (to, "shrank")
    case .grew(let to): (side, what) = (to, "grew")
    }
    do {
      renderer = try loaded.renderer(maxSide: side)
      clientLogger.notice("avatar.picture \(what, privacy: .public) side=\(side)")
    } catch let error as AvatarPackageError {
      // The same pictures drew the other size, so this is not expected: keep what draws.
      pacer = AvatarFramePacer(side: renderer.outputSide)
      clientLogger.error(
        "avatar.picture_refused kind=\(error.kind.rawValue, privacy: .public)")
    } catch {
      pacer = AvatarFramePacer(side: renderer.outputSide)
      clientLogger.error("avatar.picture_refused kind=unknown")
    }
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

  /// What a view shows in place of the avatar when the package did not load: the kind of the
  /// refusal and no path. `nil` while it is loading or drawn.
  public var failureText: String? {
    failure.map { "The avatar could not be loaded (\($0.rawValue))" }
  }

  private enum Source {
    case folder(URL)
    case loaded(AvatarLoadedPackage, seed: UInt64)
  }

  private let source: Source
  private let maxSide: Int
  private var loading: Task<AvatarStageEngine, any Error>?
  /// How many times the package was read (once, whatever the views do).
  private(set) var loadsStarted = 0
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

  /// A view says whether it shows the model now (on screen, in a scene that is not in the
  /// background, and not covered). The loop runs while any view does.
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

  /// The engine, loaded once: a view that goes and comes back while the package is still being
  /// read waits for the same load, and a package that was refused is not read again.
  private func engine() async throws -> AvatarStageEngine {
    if loading == nil {
      loadsStarted += 1
      let (source, maxSide) = (source, maxSide)
      loading = Task {
        switch source {
        case .folder(let url):
          return try await AvatarStageEngine.load(
            folder: url, seed: UInt64.random(in: UInt64.min...UInt64.max), maxSide: maxSide)
        case .loaded(let package, let seed):
          return try AvatarStageEngine(loaded: package, seed: seed, maxSide: maxSide)
        }
      }
    }
    guard let loading else { throw AvatarPackageError(kind: .malformed, at: "") }
    return try await loading.value
  }

  private func run() async {
    let engine: AvatarStageEngine
    do {
      engine = try await self.engine()
    } catch {
      let kind = (error as? AvatarPackageError)?.kind ?? .malformed
      if failure == nil {
        failure = kind
        clientLogger.error("avatar.load_failed kind=\(kind.rawValue, privacy: .public)")
      }
      return
    }
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
/// carries no colour of its own: the view that hosts it gives the background and the look of a
/// failed load (`AvatarStageModel.failureText`). It runs the model's frames while it is on screen
/// in a scene that is not in the background and `visible` (the Mac view says when its window is
/// covered), and says what the avatar is doing to VoiceOver.
public struct AvatarStageView: View {
  let model: AvatarStageModel
  let visible: Bool
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @Environment(\.scenePhase) private var scenePhase
  @State private var viewer = UUID()

  public init(model: AvatarStageModel, visible: Bool = true) {
    self.model = model
    self.visible = visible
  }

  private var showing: Bool { visible && scenePhase != .background }

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
    .accessibilityLabel(model.failureText ?? "Avatar, \(model.state.rawValue)")
    .onAppear {
      model.reducesMotion = reduceMotion
      model.setViewing(viewer, showing)
    }
    .onDisappear { model.setViewing(viewer, false) }
    .onChange(of: showing) { _, now in model.setViewing(viewer, now) }
    .onChange(of: reduceMotion) { _, reduced in model.reducesMotion = reduced }
  }
}
