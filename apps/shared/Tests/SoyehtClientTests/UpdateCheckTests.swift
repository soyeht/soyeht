import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/update.json`, which the desktop client and the release script
/// pass as well.
@Test func everySharedCaseReadsTheManifestAsRecorded() throws {
  let vector = try ContractVector.load("update")
  let cases = ContractVector.cases(vector, "cases")
  #expect(cases.count > 40)
  for item in cases {
    let name = item["name"].string ?? ""
    let keys = (item["trust"].array ?? []).map { named -> UpdatePublicKey in
      let key = vector["keys"][named.string ?? ""]
      return UpdatePublicKey(id: key["id"].string ?? "", key: key["key"].string ?? "")
    }
    let build = UpdateBuild(
      version: try #require(UpdateVersion(item["running"].string ?? "")),
      system: item["system"].string ?? "", architecture: item["architecture"].string ?? "",
      channel: item["channel"].string ?? "")
    let result = UpdateCheck.evaluate(item["manifest"].string ?? "", build: build, keys: keys)
    let expected = item["expected"]
    switch result {
    case .failure(let failure):
      #expect(expected["status"].string == "failed", "\(name)")
      #expect(expected["failure"].string == failure.rawValue, "\(name)")
    case .success(.upToDate):
      #expect(expected["status"].string == "upToDate", "\(name)")
    case .success(.available(let offer)):
      #expect(expected["status"].string == "available", "\(name)")
      #expect(expected["version"].string == offer.version, "\(name)")
      #expect(expected["required"].bool == offer.required, "\(name)")
      #expect(expected["notesUrl"].string == offer.notesURL, "\(name)")
      #expect(expected["package"]["url"].string == offer.package.url, "\(name)")
      #expect(expected["package"]["sha256"].string == offer.package.sha256, "\(name)")
      #expect(expected["package"]["size"].int.map(UInt64.init) == offer.package.size, "\(name)")
    }
  }
}

@Test func aBuildThatCarriesTheContractsKeysTrustsNoTestKey() throws {
  let vector = try ContractVector.load("update")
  let item = try #require(vector["cases"].array?.first)
  let build = try #require(UpdateBuild.current)
  let result = UpdateCheck.evaluate(item["manifest"].string ?? "", build: build)
  guard case .failure(let failure) = result else {
    Issue.record("a test key must not be trusted")
    return
  }
  #expect([UpdateFailure.notConfigured, .unknownKey].contains(failure))
}

@Test func aManifestOverTheLimitIsRefusedBeforeItIsRead() throws {
  let vector = try ContractVector.load("update")
  let item = try #require(vector["cases"].array?.first)
  let key = vector["keys"]["first"]
  let text =
    (item["manifest"].string ?? "") + String(repeating: " ", count: UpdatePolicy.manifestMaxBytes)
  let build = try #require(UpdateBuild.current)
  let result = UpdateCheck.evaluate(
    text, build: build,
    keys: [UpdatePublicKey(id: key["id"].string ?? "", key: key["key"].string ?? "")])
  #expect(result == .failure(.tooLarge))
}

@Test func versionsAreThreePlainNumbersThatCompareAsNumbers() {
  for good in ["0.0.0", "0.1.0", "10.20.30", "999.999.999"] {
    #expect(UpdateVersion(good)?.description == good)
  }
  let bad = [
    "", "1", "1.2", "1.2.3.4", "01.2.3", "1.02.3", "1.2.03", "1.2.-3", "1.2.+3", "1.2. 3",
    "v1.2.3", "1.2.3-beta", "1000.0.0", "1.2.99999999999",
  ]
  for text in bad {
    #expect(UpdateVersion(text) == nil, "\(text)")
  }
  #expect(UpdateVersion("0.10.0")! > UpdateVersion("0.9.0")!)
  #expect(UpdateVersion("1.0.0")! > UpdateVersion("0.999.999")!)
}

@Test func theBuildIsTheProductVersionOnTheMac() throws {
  let build = try #require(UpdateBuild.current)
  #expect(build.version.description == ProductVersion.current)
  #expect(build.system == "macos")
  #expect(UpdatePolicy.architectures.contains(build.architecture))
}
