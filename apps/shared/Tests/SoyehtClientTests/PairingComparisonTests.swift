import Foundation
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
