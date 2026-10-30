import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let offer = UpdateOffer(
  version: "0.2.0", required: false,
  notesURL: "https://github.com/soyeht/soyeht/releases/tag/v0.2.0",
  package: UpdatePackage(
    url: "https://github.com/soyeht/soyeht/releases/download/v0.2.0/soyeht.dmg",
    sha256: String(repeating: "a", count: 64), size: 1))

/// The cases of `contracts/vectors/update-status.json`, which the desktop client passes as well.
@Test func everySharedCaseSaysTheSameSentences() throws {
  let cases = ContractVector.cases(try ContractVector.load("update-status"), "cases")
  #expect(cases.count >= 8)
  for item in cases {
    let name = item["name"].string ?? ""
    let status = item["status"]
    let state: UpdateStatus
    switch status["kind"].string {
    case "unknown": state = .unknown
    case "checking": state = .checking
    case "upToDate": state = .upToDate
    case "available":
      state = .available(
        UpdateOffer(
          version: status["version"].string ?? "", required: status["required"].bool ?? false,
          notesURL: "", package: offer.package))
    default:
      state = .failed(try #require(UpdateFailure(rawValue: status["failure"].string ?? "")))
    }
    let made = UpdatePresentation.make(
      enabled: item["enabled"].bool ?? false, status: state, running: item["running"].string ?? "")
    let expected = item["expected"]
    #expect(made.headline == expected["headline"].string, "\(name)")
    #expect(made.warning == expected["warning"].string, "\(name)")
    #expect(made.download == expected["download"].string, "\(name)")
    #expect(made.badge == expected["badge"].bool, "\(name)")
  }
}

@MainActor private func checker(
  keys: [UpdatePublicKey] = [UpdatePublicKey(id: "k", key: "")],
  preferences: MemoryPreferenceStore = MemoryPreferenceStore(),
  fetch: @escaping UpdateFetch
) -> UpdateChecker {
  UpdateChecker(
    build: UpdateVersion("0.1.0").map {
      UpdateBuild(version: $0, system: "macos", architecture: "aarch64", channel: "stable")
    }, keys: keys, preferences: preferences, fetch: fetch, interval: .seconds(86400))
}

@Test @MainActor func aBuildWithNoKeyAsksNobody() async {
  let asked = AskedCounter()
  let updates = checker(keys: []) { _ in
    await asked.add()
    return .failure(.unreachable)
  }
  await updates.checkNow()
  #expect(updates.status == .failed(.notConfigured))
  #expect(await asked.count == 0)
}

@Test @MainActor func aFetchThatFailsIsAFailureAndTextThatIsNoManifestIsRefused() async {
  let unreachable = checker { _ in .failure(.unreachable) }
  await unreachable.checkNow()
  #expect(unreachable.status == .failed(.unreachable))
  #expect(unreachable.offer == nil)
  let portal = checker { _ in .success("<html>captive portal</html>") }
  await portal.checkNow()
  #expect(portal.status == .failed(.badEnvelope))
}

@Test @MainActor func theAddressAskedIsTheContractsAndTheCheckIsOnUntilTurnedOff() async throws {
  let store = MemoryPreferenceStore()
  let asked = AskedCounter()
  let updates = checker(preferences: store) { address in
    await asked.add(address.absoluteString)
    return .failure(.unreachable)
  }
  #expect(updates.enabled)
  await updates.checkNow()
  #expect(await asked.addresses == [UpdatePolicy.manifestURL])
  updates.setEnabled(false)
  #expect(store.values[UpdateChecker.enabledKey] == false)
  await updates.checkNow()
  #expect(await asked.count == 1)
  #expect(updates.status == .unknown)
  #expect(updates.presentation.headline == UpdateText.off)
  #expect(!updates.presentation.badge)
}

@Test @MainActor func theChoiceOfTheLastLaunchIsKept() {
  let store = MemoryPreferenceStore()
  store.set(false, forKey: UpdateChecker.enabledKey)
  #expect(!checker(preferences: store) { _ in .failure(.unreachable) }.enabled)
}

private actor AskedCounter {
  private(set) var addresses: [String] = []
  var count: Int { addresses.count }
  func add(_ address: String = "") { addresses.append(address) }
}
