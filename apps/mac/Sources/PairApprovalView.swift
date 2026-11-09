import SoyehtClient
import SwiftUI

/// The one approval of a request to pair, used wherever the Mac can show one (the bar over the
/// window and the pairing sheet; ADR 0152). It always shows the comparison code the new device
/// shows, and what to do with it: no approval offers Allow without the code. A request whose
/// fingerprint has no code cannot be compared, so it can only be denied.
struct PairApprovalView: View {
  let request: PairPrompt
  /// `true` when the person allowed the request, `false` when they denied it.
  let decide: (Bool) -> Void

  var body: some View {
    VStack(spacing: MacTheme.Space.x12) {
      Text(PairingCopy.allowQuestion(request.deviceName))
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      if let code = request.comparisonCode {
        Text(code).font(MacTheme.mono(MacTheme.TypeScale.heading, weight: .semibold))
          .textSelection(.enabled)
          .accessibilityIdentifier("pair-comparison-code")
        Text(PairingCopy.compareOnMac).font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary).multilineTextAlignment(.center)
      } else {
        Text(PairingCopy.noComparisonCode).font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.danger).multilineTextAlignment(.center)
      }
      HStack {
        Button("Deny") { decide(false) }.buttonStyle(.paneSecondary)
        Button("Allow") { decide(true) }.buttonStyle(.panePrimary)
          .disabled(request.comparisonCode == nil)
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("pair-approval")
  }
}
