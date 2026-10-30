import Foundation
import Observation
import SoyehtProtocol
import os

/// What the last check found.
public enum UpdateStatus: Equatable, Sendable {
  /// No check has ended yet.
  case unknown
  case checking
  case upToDate
  case available(UpdateOffer)
  case failed(UpdateFailure)
}

/// What the Mac says about the update check. The desktop client's `Updates::headline` says the
/// same; `contracts/vectors/update-status.json` checks both.
public struct UpdatePresentation: Equatable, Sendable {
  public let headline: String
  /// This version is no longer supported.
  public let warning: String?
  /// The label of the button that opens the package.
  public let download: String?
  /// The Settings entry carries a dot: a newer version is on offer.
  public let badge: Bool

  public static func make(enabled: Bool, status: UpdateStatus, running: String)
    -> UpdatePresentation
  {
    func about(_ sentence: String, _ version: String) -> String {
      sentence.replacingOccurrences(of: "{version}", with: version)
    }
    guard enabled else {
      return UpdatePresentation(
        headline: UpdateText.off, warning: nil, download: nil, badge: false)
    }
    let quiet = { (headline: String) in
      UpdatePresentation(headline: headline, warning: nil, download: nil, badge: false)
    }
    switch status {
    case .unknown: return quiet(UpdateText.notChecked)
    case .checking: return quiet(UpdateText.checking)
    case .upToDate: return quiet(about(UpdateText.upToDate, running))
    case .failed(.notConfigured): return quiet(UpdateText.notConfigured)
    case .failed: return quiet(UpdateText.failed)
    case .available(let offer):
      return UpdatePresentation(
        headline: about(UpdateText.available, offer.version),
        warning: offer.required ? UpdateText.required : nil,
        download: about(UpdateText.download, offer.version), badge: true)
    }
  }
}

/// Fetches the manifest at an address: its text, or why that was not possible.
public typealias UpdateFetch = @Sendable (URL) async -> Result<String, UpdateFailure>

/// Counts the redirects of one fetch and refuses the one past the contract's limit, or one that
/// leaves HTTPS.
private final class RedirectLimit: NSObject, URLSessionTaskDelegate, Sendable {
  private let followed = OSAllocatedUnfairLock(initialState: 0)

  func urlSession(
    _ session: URLSession, task: URLSessionTask,
    willPerformHTTPRedirection response: HTTPURLResponse,
    newRequest request: URLRequest
  ) async -> URLRequest? {
    let count = followed.withLock { value -> Int in
      value += 1
      return value
    }
    guard count <= UpdatePolicy.maxRedirects, request.url?.scheme == "https" else { return nil }
    return request
  }
}

/// The fetch of the app: HTTPS through the system, never more than the manifest's size, with the
/// contract's one `User-Agent`. The answer is not trusted (a key of the contract has to have
/// signed it), so the system's own certificate checks are all this adds.
public func fetchUpdateManifest(_ address: URL) async -> Result<String, UpdateFailure> {
  await fetchManifest(address, configuration: .ephemeral)
}

/// The same fetch over a given session configuration, so a test can put a `URLProtocol` stub in
/// front of the network. The redirect rules and the limits are the ones of the app's fetch.
func fetchManifest(_ address: URL, configuration: URLSessionConfiguration) async
  -> Result<String, UpdateFailure>
{
  guard address.scheme == "https" else { return .failure(.unreachable) }
  configuration.timeoutIntervalForRequest = TimeInterval(UpdatePolicy.checkTimeoutSeconds)
  configuration.timeoutIntervalForResource = TimeInterval(UpdatePolicy.checkTimeoutSeconds)
  let session = URLSession(configuration: configuration)
  defer { session.finishTasksAndInvalidate() }
  var request = URLRequest(url: address)
  request.setValue(UpdatePolicy.userAgent, forHTTPHeaderField: "User-Agent")
  do {
    let (bytes, response) = try await session.bytes(for: request, delegate: RedirectLimit())
    guard (response as? HTTPURLResponse)?.statusCode == 200 else { return .failure(.unreachable) }
    var body = Data()
    for try await byte in bytes {
      body.append(byte)
      if body.count > UpdatePolicy.manifestMaxBytes { return .failure(.tooLarge) }
    }
    return String(data: body, encoding: .utf8).map { .success($0) } ?? .failure(.badEnvelope)
  } catch {
    return .failure(.unreachable)
  }
}

/// Whether this app asks for a newer version, and what it found (ADR 0122): once at start, then
/// each day while the app runs. It only tells; the person's browser downloads the package.
@MainActor @Observable
public final class UpdateChecker {
  public static let enabledKey = "soyeht.updates.check"
  /// The highest sequence number of a manifest this device has read (ADR 0122).
  public static let sequenceKey = "soyeht.updates.sequence"

  public private(set) var enabled: Bool
  public private(set) var status: UpdateStatus = .unknown
  @ObservationIgnored private let build: UpdateBuild?
  @ObservationIgnored private let keys: [UpdatePublicKey]
  @ObservationIgnored private let preferences: any ClientPreferenceStore
  @ObservationIgnored private let fetch: UpdateFetch
  @ObservationIgnored private let interval: Duration
  @ObservationIgnored private var loop: Task<Void, Never>?
  /// A check is running, whatever the switch did meanwhile: one at a time.
  @ObservationIgnored private var inFlight = false
  /// The failure the log last said, so a check that fails the same way each day says it once.
  @ObservationIgnored private var logged: UpdateFailure?

  public convenience init() {
    self.init(
      build: .current, keys: UpdatePolicy.publicKeys,
      preferences: UserDefaultsClientPreferenceStore(), fetch: fetchUpdateManifest,
      interval: .seconds(UpdatePolicy.checkIntervalSeconds))
  }

  public init(
    build: UpdateBuild?, keys: [UpdatePublicKey], preferences: any ClientPreferenceStore,
    fetch: @escaping UpdateFetch, interval: Duration
  ) {
    self.build = build
    self.keys = keys
    self.preferences = preferences
    self.fetch = fetch
    self.interval = interval
    enabled = preferences.storedBool(forKey: Self.enabledKey) ?? true
  }

  public var presentation: UpdatePresentation {
    UpdatePresentation.make(enabled: enabled, status: status, running: ProductVersion.current)
  }

  /// The offer a newer version makes, while the check is on.
  public var offer: UpdateOffer? {
    if enabled, case .available(let offer) = status { return offer }
    return nil
  }

  /// Starts the daily check; the first one runs at once. Calling it again does nothing.
  public func start() {
    guard loop == nil else { return }
    let interval = interval
    loop = Task { [weak self] in
      while self != nil {
        await self?.checkNow()
        do { try await Task.sleep(for: interval) } catch { break }
      }
    }
  }

  public func setEnabled(_ enabled: Bool) {
    self.enabled = enabled
    preferences.set(enabled, forKey: Self.enabledKey)
    if enabled {
      if inFlight { status = .checking }
      Task { await checkNow() }
    } else {
      status = .unknown
    }
  }

  /// Asks now, unless the check is off or one is already running.
  public func checkNow() async {
    guard enabled, !inFlight else { return }
    guard let build else {
      status = .failed(.unknownBuild)
      return
    }
    inFlight = true
    status = .checking
    let floor = UInt64(max(preferences.integer(forKey: Self.sequenceKey) ?? 0, 0))
    let result = await Self.read(build: build, keys: keys, floor: floor, fetch: fetch)
    inFlight = false
    guard enabled else { return }
    switch result {
    case .success(let checked):
      if checked.sequence > floor {
        preferences.set(Int(clamping: checked.sequence), forKey: Self.sequenceKey)
      }
      switch checked.outcome {
      case .upToDate: status = .upToDate
      case .available(let offer): status = .available(offer)
      }
      logged = nil
    case .failure(let failure):
      if logged != failure {
        clientLogger.notice("mac.update_check_failed kind=\(failure.rawValue, privacy: .public)")
      }
      logged = failure
      status = .failed(failure)
    }
  }

  private nonisolated static func read(
    build: UpdateBuild, keys: [UpdatePublicKey], floor: UInt64, fetch: UpdateFetch
  ) async -> Result<UpdateChecked, UpdateFailure> {
    // A build with no key asks nobody: it could not trust the answer.
    guard !keys.isEmpty, let address = URL(string: UpdatePolicy.manifestURL) else {
      return .failure(.notConfigured)
    }
    switch await fetch(address) {
    case .success(let text):
      return UpdateCheck.evaluate(text, build: build, keys: keys, floor: floor)
    case .failure(let failure): return .failure(failure)
    }
  }
}
