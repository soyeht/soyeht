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
    private let supervisorRegistered: Bool
    private let failure: ServiceCommandError?

    /// `supervisorRegistered` is whether the supervisor is registered when the engine is (a
    /// registration that lost one file has only one of them).
    init(
      registered: Bool, current: Bool = false, supervisorRegistered: Bool = true,
      failure: ServiceCommandError? = nil
    ) {
      self.registered = registered
      self.current = current
      self.supervisorRegistered = supervisorRegistered
      self.failure = failure
    }

    private func state(_ component: String, registered: Bool) -> String {
      #"{"component":"\#(component)","name":"n","registered":\#(registered),"running":false,"current":\#(current)}"#
    }

    var asked: [[String]] { calls.withLock { $0 } }

    func run(_ executable: URL, arguments: [String]) async throws -> Data {
      calls.withLock { $0.append(arguments) }
      if let failure, arguments.contains("install") { throw failure }
      let states = [
        state("ptyd", registered: registered && supervisorRegistered),
        state("engine", registered: registered),
      ]
      return Data(#"{"services":[\#(states.joined(separator: ","))]}"#.utf8)
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

  /// The setup after the app's first look at the services.
  @MainActor private func launched(_ runner: ScriptedRunner) async -> EngineServiceSetup {
    let setup = EngineServiceSetup(service: service(runner), preferences: MemoryPreferences())
    await setup.refresh()
    return setup
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
    let setup = await launched(runner)
    #expect(setup.phase == .ready)
    #expect(runner.asked.last?.contains("install") == true)
    #expect(!setup.shouldAsk)
  }

  @MainActor @Test func aRegistrationThatIsRightIsLeftAloneSoAStoppedEngineStaysStopped() async {
    let runner = ScriptedRunner(registered: true, current: true)
    let setup = await launched(runner)
    #expect(setup.phase == .ready)
    #expect(runner.asked.count == 1, "only the status was asked")
    #expect(runner.asked[0].last == "/Applications/Soyeht Next.app/Contents/MacOS")
  }

  @MainActor @Test func registeredAndRightServicesThatDoNotAnswerGetTheButtonAndNoSilentStart()
    async
  {
    let runner = ScriptedRunner(registered: true, current: true)
    let setup = await launched(runner)
    #expect(setup.phase == .ready)
    #expect(setup.offersStart, "the setup list offers Start Soyeht while the engine is silent")
    #expect(!setup.shouldAsk, "the question is only for a first run")
    #expect(runner.asked.allSatisfy { !$0.contains("install") }, "a stop is respected at launch")
    await setup.enable()
    #expect(runner.asked.last?.contains("install") == true, "the button runs install")
  }

  @MainActor @Test func aRegistrationThatLostOneServiceIsRepairedLikeAStaleOne() async {
    let runner = ScriptedRunner(registered: true, current: true, supervisorRegistered: false)
    let setup = await launched(runner)
    #expect(setup.phase == .ready)
    #expect(runner.asked.last?.contains("install") == true)
    let none = ScriptedRunner(registered: false, supervisorRegistered: false)
    let fresh = await launched(none)
    #expect(fresh.phase == .offered, "nothing registered is a first run, not a repair")
    #expect(none.asked.allSatisfy { !$0.contains("install") })
  }

  @MainActor @Test func agreeingRegistersAndAFailureShowsWhatTheCommandSaid() async {
    let failing = ScriptedRunner(
      registered: false, failure: .failed(status: 1, detail: "A program is missing"))
    let setup = await launched(failing)
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
