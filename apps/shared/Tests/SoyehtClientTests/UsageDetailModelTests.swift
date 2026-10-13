import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private var colors: SoyehtThemeColorSet {
  SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette)
}

private func reading(
  _ state: UsageState, windows: [(String, Double)] = [], reason: UsageUnavailableReason? = nil
) -> UsageReading {
  UsageReading(
    provider: .codex, state: state, unavailableReason: reason,
    primary: windows.first.map { UsageWindow(used: $0.1, resetsAt: "later", label: $0.0) },
    secondary: windows.dropFirst().first.map {
      UsageWindow(used: $0.1, resetsAt: "later", label: $0.0)
    },
    source: .codex_app_server, fetchedAt: "2026-09-12T12:00:00Z")
}

@Test func aCurrentReadingListsItsWindowsWithTheirBandsAndNoHint() {
  let model = UsageDetailModel(
    reading(.ok, windows: [("5-hour", 0.9), ("Weekly", 0.3)]), onThisMac: true)
  #expect(model.providerName == "Codex")
  #expect(model.isCurrent)
  #expect(model.statusColor(in: colors) == colors.success)
  guard case .windows(let rows, let hint) = model.content else {
    Issue.record("expected the windows")
    return
  }
  #expect(hint == nil)
  #expect(rows.map(\.percentage) == ["90%", "30%"])
  #expect(rows.map(\.band) == [.danger, .success])
  #expect(rows.map(\.barOpacity) == [1, 1])
  #expect(model.footer.hasPrefix("Source: Codex app server · "))
}

@Test func aStaleReadingMutesItsBarsAndSaysWhyItWasNotReplaced() {
  let model = UsageDetailModel(
    reading(.stale, windows: [("Weekly", 0.5)], reason: .rate_limited), onThisMac: true)
  #expect(!model.isCurrent)
  guard case .windows(let rows, let hint) = model.content else {
    Issue.record("expected the windows")
    return
  }
  #expect(rows.map(\.barOpacity) == [UsageEmphasis.lastKnown.arcOpacity])
  #expect(hint == "Latest refresh: Codex is limiting usage requests; retrying later.")
}

@Test func aReadingThatCannotBeShownGivesOneNotice() {
  func notice(_ reading: UsageReading, onThisMac: Bool = true) -> String? {
    guard case .notice(let notice) = UsageDetailModel(reading, onThisMac: onThisMac).content
    else { return nil }
    return notice.message
  }
  #expect(notice(reading(.login_needed)) == "Sign in in your terminal.")
  #expect(notice(reading(.no_access)) == "Allow access in Settings.")
  #expect(notice(reading(.no_access), onThisMac: false) == "Allow access in Settings on your Mac.")
  #expect(notice(reading(.ok)) == "No reading yet.")
  #expect(notice(reading(.stale)) == "Last reading is stale.")
  #expect(
    notice(reading(.unavailable, reason: .tool_not_found)) == "Codex not found on this Mac.")
}

@Test func theStatusDotIsTheSameColourOnTheMacAndThePhone() {
  for state in [UsageState.ok, .stale, .login_needed, .no_access, .unavailable] {
    let colours = [true, false].map {
      UsageDetailModel(reading(state, windows: [("Weekly", 0.5)]), onThisMac: $0)
        .statusColor(in: colors)
    }
    #expect(colours[0] == colours[1], "\(state)")
    #expect(colours[0] == (state == .ok ? colors.success : colors.muted), "\(state)")
  }
}

#if os(macOS)
  import SwiftUI

  /// The five states a reading can be in, drawn as the status dot of the Mac balloon and the
  /// iPhone sheet: a current reading is `success`, every other one `muted`, and none is `warning`.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func theStatusDotsRenderInThePaletteRoles(theme: TerminalColorTheme) throws {
    let palette = theme.palette
    let set = SoyehtThemeColorSet(palette: palette)
    let states: [UsageState] = [.ok, .stale, .login_needed, .no_access, .unavailable]
    let dots = HStack(spacing: 14) {
      ForEach(states, id: \.self) { state in
        Circle()
          .fill(
            UsageDetailModel(reading(state, windows: [("Weekly", 0.5)]), onThisMac: false)
              .statusColor(in: set)
          )
          .frame(width: 18, height: 18)
      }
    }
    .padding(DesignTokens.Space.x14)
    .background(set.face)
    let pixels = try PaneInteriorRendering.shot(
      "usage-status-dots", dots, size: CGSize(width: 5 * 18 + 4 * 14 + 28, height: 46),
      theme: theme)
    #expect(PaneInteriorRendering.share(of: palette.success, in: pixels) > 0, "\(theme) success")
    #expect(PaneInteriorRendering.share(of: palette.muted, in: pixels) > 0, "\(theme) muted")
    #expect(PaneInteriorRendering.share(of: palette.warning, in: pixels) == 0, "\(theme) warning")
  }
#endif
