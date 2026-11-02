import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The quiet notice "Soyeht closed unexpectedly" (ADR 0127, contract 16): what happened, that
/// nothing was sent, "Copy report" (which is Copy diagnostics, since the report is part of it)
/// and "Dismiss". The desktop app shows the same sentences.
struct CrashNoticeView: View {
  @Environment(CrashNotice.self) private var notice
  @Environment(DiagnosticsCopy.self) private var diagnostics

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
      Text(ClientPolicy.crashNoticeTitle)
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      Text(ClientPolicy.crashNoticeSummary)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
      DiagnosticsMessage(identifier: "crash-notice")
      HStack(spacing: MacTheme.Space.x10) {
        Spacer()
        Button(ClientPolicy.crashNoticeDismiss) { Task { await notice.dismiss() } }
          .buttonStyle(.paneSecondary)
          .accessibilityIdentifier("crash-notice-dismiss")
        Button(ClientPolicy.crashNoticeCopy) {
          Task { await notice.copyReport(using: diagnostics) }
        }
        .buttonStyle(.panePrimary)
        .disabled(diagnostics.phase == .working)
        .accessibilityIdentifier("crash-notice-copy")
      }
    }
    .padding(MacTheme.Space.x18)
    .frame(width: 380)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("crash-notice")
  }
}
