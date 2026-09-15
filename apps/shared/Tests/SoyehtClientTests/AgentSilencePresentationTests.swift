import Foundation
import Testing

@testable import SoyehtClient

@Test func silencePresentationShowsQuietWaitingThenEscalatesAndHandlesClockJumps() throws {
  let start = try #require(UsageTimestamp.date(from: "2026-09-15T18:00:00.000Z"))
  let after = start.addingTimeInterval(154)
  let presentation = AgentSilencePresentation(
    silentSince: "2026-09-15T18:00:00.000Z", now: after)
  #expect(presentation.elapsedSeconds == 154)
  #expect(presentation.elapsed == "2m 34s")
  #expect(presentation.escalated)

  let quiet = AgentSilencePresentation(
    silentSince: "2026-09-15T18:02:31.000Z", now: after)
  #expect(quiet.elapsedSeconds == 3)
  #expect(quiet.elapsed == "0m 3s")
  #expect(!quiet.escalated)

  let future = AgentSilencePresentation(
    silentSince: "2026-09-15T18:03:00.000Z", now: after)
  #expect(future.elapsedSeconds == 0)
  #expect(!future.escalated)

  let slept = AgentSilencePresentation(
    silentSince: "2026-09-15T18:00:00.000Z", now: start.addingTimeInterval(8 * 3_600))
  #expect(slept.elapsed == "8h 0m")
}
