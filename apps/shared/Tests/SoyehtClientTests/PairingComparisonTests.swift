import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/pairing-comparison.json`, which `soyeht-protocol` passes as
/// well: the short code the new device and the Mac show for the person to compare (ADR 0152).
@Test func everyCaseOfTheComparisonVectorGivesTheSameCode() throws {
  let vector = try ContractVector.load("pairing-comparison")
  let cases = try #require(vector["cases"].array)
  #expect(cases.count >= 5)
  for item in cases {
    let name = item["name"].string ?? ""
    #expect(
      PairingCopy.comparisonCode(item["fingerprint"].string ?? "") == item["code"].string,
      "\(name)")
  }
}

@Test func theMacAndTheDeviceSayWhatToDoWithTheCode() {
  #expect(PairingCopy.compareOnMac.contains("deny"))
  #expect(PairingCopy.compareOnDevice.contains("Cancel"))
}

@Test func aRequestWhoseFingerprintHasNoCodeHasNoCodeToShow() throws {
  func prompt(_ fingerprint: String) throws -> PairPrompt {
    let json =
      #"{"kind":"device_pair","requestId":"r","deviceName":"Phone","fingerprint":"\#(fingerprint)","expiresAt":"2026-11-07T00:00:00Z"}"#
    return PairPrompt(try JSONDecoder().decode(DevicePairPending.self, from: Data(json.utf8)))
  }
  let full = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
  #expect(try prompt(full).comparisonCode == "E3B0 C442 98FC 1C14")
  #expect(try prompt("e3b0c44298fc").comparisonCode == nil)
}

@Test func theQuestionNamesTheDeviceAndNoCopyAsksToCompareTheFingerprint() {
  #expect(PairingCopy.allowQuestion("Phone") == "Allow access for Phone?")
  for text in [
    PairingCopy.waitingForTheMac, PairingCopy.compareOnDevice, PairingCopy.compareOnMac,
    PairingCopy.noComparisonCode,
  ] {
    #expect(!text.lowercased().contains("fingerprint"))
  }
}
