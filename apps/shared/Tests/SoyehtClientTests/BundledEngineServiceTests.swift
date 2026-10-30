#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import os
  import Testing

  @testable import SoyehtClient

  /// Answers each `soyeht service` call from a script and records what was asked.
  private final class ScriptedRunner: ServiceCommandRunning, Sendable {
    private let calls = OSAllocatedUnfairLock<[[String]]>(initialState: [])
    private let registered: Bool
    private let current: Bool
    private let failure: ServiceCommandError?

    init(registered: Bool, current: Bool = false, failure: ServiceCommandError? = nil) {
      self.registered = registered
      self.current = current
      self.failure = failure
    }

    var asked: [[String]] { calls.withLock { $0 } }

    func run(_ executable: URL, arguments: [String]) async throws -> Data {
      calls.withLock { $0.append(arguments) }
      if let failure, arguments.contains("install") { throw failure }
      let state =
        #"{"component":"engine","name":"n","registered":\#(registered),"running":false,"current":\#(current)}"#
      return Data(#"{"services":[\#(state)]}"#.utf8)
    }
  }

  private final class MemoryPreferences: ClientPreferenceStore {
    var values: [String: Bool] = [:]
    func bool(forKey key: String) -> Bool { values[key] ?? false }
    func storedBool(forKey key: String) -> Bool? { values[key] }
    func set(_ value: Bool, forKey key: String) { values[key] = value }
    func integer(forKey key: String) -> Int? { nil }
    func set(_ value: Int, forKey key: String) {}
  }

  private let bundledCommand = URL(
    fileURLWithPath: "/Applications/Soyeht Next.app/Contents/MacOS/soyeht")
  private let data = URL(fileURLWithPath: "/Users/person/Library/Application Support/Soyeht.next")

  private func service(_ runner: ScriptedRunner, command: URL = bundledCommand)
    -> BundledEngineService
  {
    BundledEngineService(command: command, profile: .prod, dataDirectory: data, runner: runner)
  }

  @Test func theBundledCommandRunsForTheProfileWithTheBundlesOwnPrograms() async throws {
    let runner = ScriptedRunner(registered: false)
    let bundled = service(runner)
    _ = try await bundled.status()
    try await bundled.install()
    let prefix = ["--profile", "prod", "--data-dir", data.path, "service"]
    #expect(
      runner.asked == [
        prefix + ["status", "--programs", "/Applications/Soyeht Next.app/Contents/MacOS"],
        prefix + ["install", "--programs", "/Applications/Soyeht Next.app/Contents/MacOS"],
      ])
  }

  @Test func aCopyOnADiskImageOrTranslocatedIsNotAStablePlace() {
    #expect(BundledEngineService.isStablePlace(bundledCommand))
    #expect(BundledEngineService.isStablePlace(URL(fileURLWithPath: "/Users/p/Applications/S.app")))
    #expect(!BundledEngineService.isStablePlace(URL(fileURLWithPath: "/Volumes/Soyeht Next/S.app")))
    #expect(
      !BundledEngineService.isStablePlace(
        URL(fileURLWithPath: "/private/var/folders/x/AppTranslocation/ABC/d/S.app")))
  }

  @Test func theRunnerReturnsWhatAProgramPrintedAndWhatAFailedOneSaid() async throws {
    let printed = try await ProcessRunner().run(
      URL(fileURLWithPath: "/bin/echo"), arguments: ["services"])
    #expect(String(decoding: printed, as: UTF8.self) == "services\n")
    await #expect(throws: ServiceCommandError.failed(status: 1, detail: "")) {
      try await ProcessRunner().run(URL(fileURLWithPath: "/usr/bin/false"), arguments: [])
    }
  }

  @MainActor @Test func aFreshInstallOffersTheStartAndAsksOnce() async {
    let runner = ScriptedRunner(registered: false)
    let preferences = MemoryPreferences()
    let setup = EngineServiceSetup(service: service(runner), preferences: preferences)
    await setup.refresh()
    #expect(setup.phase == .offered)
    #expect(setup.shouldAsk)
    #expect(runner.asked.count == 1, "nothing is registered before the person agrees")
    setup.decline()
    #expect(!setup.shouldAsk)
    #expect(setup.offersStart, "the setup list keeps the button after a no")
  }

  @MainActor @Test func servicesRegisteredForAnotherPathAreWrittenAgainAtLaunch() async {
    let runner = ScriptedRunner(registered: true, current: false)
    let setup = EngineServiceSetup(service: service(runner), preferences: MemoryPreferences())
    await setup.refresh()
    #expect(setup.phase == .ready)
    #expect(runner.asked.last?.contains("install") == true)
    #expect(!setup.shouldAsk)
  }

  @MainActor @Test func aRegistrationThatIsRightIsLeftAloneSoAStoppedEngineStaysStopped() async {
    let runner = ScriptedRunner(registered: true, current: true)
    let setup = EngineServiceSetup(service: service(runner), preferences: MemoryPreferences())
    await setup.refresh()
    #expect(setup.phase == .ready)
    #expect(runner.asked.count == 1, "only the status was asked")
    #expect(runner.asked[0].last == "/Applications/Soyeht Next.app/Contents/MacOS")
  }

  @MainActor @Test func agreeingRegistersAndAFailureShowsWhatTheCommandSaid() async {
    let failing = ScriptedRunner(
      registered: false, failure: .failed(status: 1, detail: "A program is missing"))
    let setup = EngineServiceSetup(service: service(failing), preferences: MemoryPreferences())
    await setup.refresh()
    await setup.enable()
    #expect(setup.phase == .failed("A program is missing"))
    #expect(setup.offersStart)
    let working = ScriptedRunner(registered: false)
    let again = EngineServiceSetup(service: service(working), preferences: MemoryPreferences())
    await again.enable()
    #expect(again.phase == .ready)
  }

  @MainActor @Test func aDiskImageCopyAsksToMoveFirstAndRegistersNothing() async {
    let runner = ScriptedRunner(registered: false)
    let command = URL(fileURLWithPath: "/Volumes/Soyeht Next/Soyeht Next.app/Contents/MacOS/soyeht")
    let setup = EngineServiceSetup(
      service: service(runner, command: command), preferences: MemoryPreferences())
    await setup.refresh()
    await setup.enable()
    #expect(setup.phase == .moveFirst)
    #expect(runner.asked.isEmpty)
  }

  @MainActor @Test func aBuildWithoutBundledProgramsHasNothingToOffer() async {
    let setup = EngineServiceSetup(service: nil, preferences: MemoryPreferences())
    await setup.refresh()
    #expect(setup.phase == .unavailable)
    #expect(!setup.shouldAsk)
    #expect(!setup.offersStart)
  }
#endif
