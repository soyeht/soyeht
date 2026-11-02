import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// Only a Dev build names its profile on screen; the production build says nothing of it.
@Suite struct ProfileNoteTests {
  @Test func aDevBuildNamesItsProfile() {
    let dev = ProfileValues.dev
    #expect(dev.note == "Dev profile")
    #expect(dev.versionLine(version: "0.1.0") == "Soyeht 0.1.0 · Dev profile")
    #expect(dev.connectingText == "Connecting to the Dev profile…")
    #expect(dev.connectedText == "Dev profile · connected")
  }

  @Test func theProductionBuildSaysNothingOfItsProfile() {
    let prod = ProfileValues.prod
    #expect(prod.note == nil)
    #expect(prod.versionLine(version: "0.1.0") == "Soyeht 0.1.0")
    #expect(prod.connectingText == "Connecting to the engine…")
    #expect(prod.connectedText == "Connected")
  }
}
