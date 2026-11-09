import SoyehtClient
import SwiftUI

/// The state of a workspace whose folder is gone: what is missing and where, and the one choice
/// there is, to close the workspace (the engine has no method to point it at another folder). It
/// stays over the window while the workspace is shown; it is not a dialog, so it does not return
/// at every launch to be dismissed.
struct MissingWorkspaceFolderBanner: View {
  let state: MissingWorkspaceFolder
  let close: () -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x12) {
      Text(state.text)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.ink)
        .textSelection(.enabled)
        .lineLimit(2)
      Button(state.closeTitle, action: close)
        .buttonStyle(.paneSecondary)
        .accessibilityIdentifier("missing-folder-close")
    }
    .padding(MacTheme.Space.x14)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("missing-folder")
  }
}
