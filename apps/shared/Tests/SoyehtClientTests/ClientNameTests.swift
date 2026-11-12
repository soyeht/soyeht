import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func aDebugBuildSendsTheDevNameAndAReleaseBuildThePlainOne() {
  #if DEBUG
    #expect(ClientName.mac == ClientPolicy.clientNameMacDev)
    #expect(ClientName.iphone == ClientPolicy.clientNameIphoneDev)
  #else
    #expect(ClientName.mac == ClientPolicy.clientNameMac)
    #expect(ClientName.iphone == ClientPolicy.clientNameIphone)
  #endif
  #expect(ClientPolicy.clientNameMacDev == ClientPolicy.clientNameMac + "-dev")
  #expect(ClientPolicy.clientNameIphoneDev == ClientPolicy.clientNameIphone + "-dev")
}

@Test func theProtocolVersionTheClientSpeaksIsTheContractsOne() {
  #expect(ProtocolVersion.current == 1)
  #expect(ProtocolVersion.min == 1)
}

@Test func theVersionRuleGivesTheAnswersOfTheVector() throws {
  let vector = try ContractVector.load("protocol-version")
  for item in ContractVector.cases(vector, "welcome") {
    let welcomed = UInt32(item["welcomed"].int ?? -1)
    let accepted = ProtocolVersion.accepts(
      welcomed, min: UInt32(item["min"].int ?? 0), current: UInt32(item["current"].int ?? 0))
    #expect(accepted == (item["accepted"] == .bool(true)), "\(item["name"].string ?? "")")
  }
}
