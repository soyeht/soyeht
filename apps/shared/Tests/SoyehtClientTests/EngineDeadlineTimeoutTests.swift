import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Suite struct EngineDeadlineTimeoutTests {
  @Test func aRequestTheEngineEndsWaitsForItsDeadlinePlusTheMargin() {
    let margin = ClientPolicy.engineDeadlineMarginSeconds
    #expect(margin > 0)
    #expect(
      engineDeadlineTimeout(ProtocolLimits.harnessSwitchTimeoutSec)
        == Double(ProtocolLimits.harnessSwitchTimeoutSec) + margin)
    #expect(
      engineDeadlineTimeout(ProtocolLimits.noSignalSec)
        == Double(ProtocolLimits.noSignalSec) + margin)
  }
}
