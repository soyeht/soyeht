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
}
