import SoyehtClient
import SwiftUI

/// Stands in for a list the engine has not sent yet. A slow or failed load is a state of
/// the list with a retry, never an alert that takes the next click.
struct HarnessLoadStateRow: View {
  enum State: Equatable {
    case loading(String)
    case failed(String)
  }

  /// Where the row sits: the Preferences list is raised, a picker's interior is flat.
  enum Surface {
    case preference
    case pane
  }

  let state: State
  var surface = Surface.preference
  /// The engine's own reason for a failure, offered as the row's tooltip.
  var detail: String?
  let retry: () -> Void

  var body: some View {
    switch surface {
    case .preference: content.preferenceRowSurface()
    case .pane:
      content
        .padding(.horizontal, MacTheme.Space.x10).frame(minHeight: 30)
        .paneWell(radius: MacTheme.Radius.compact)
    }
  }

  private var content: some View {
    HStack(spacing: MacTheme.Space.x10) {
      if case .loading = state { ProgressView().controlSize(.small) }
      Text(message)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Spacer()
      if case .failed = state {
        retryButton
      }
    }
    .help(detail ?? "")
  }

  private var message: String {
    switch state {
    case .loading(let text), .failed(let text): text
    }
  }

  @ViewBuilder private var retryButton: some View {
    switch surface {
    case .preference:
      Button("Retry", action: retry)
        .buttonStyle(NeoPillButtonStyle())
        .accessibilityIdentifier("harness-load-retry")
    case .pane:
      Button("Retry", action: retry)
        .buttonStyle(.paneSecondary)
        .accessibilityIdentifier("harness-load-retry")
    }
  }
}

/// The Harnesses list's load states: the placeholder while the list is empty, and a retry
/// row for each side list that failed to load while the engine is connected.
struct HarnessListLoadRows: View {
  let client: EngineClient

  var body: some View {
    if client.harnesses.isEmpty {
      HarnessLoadStateRow(
        state: listState, detail: client.loadFailure(.harnesses),
        retry: { client.refreshHarnesses() })
    }
    failureRow(.customHarnesses) { client.refreshCustomHarnesses() }
    failureRow(.defaultPermissionModes) { client.refreshDefaultPermissionModes() }
  }

  private var listState: HarnessLoadStateRow.State {
    if !client.connected { return .loading("Waiting for the engine…") }
    if client.harnessListStalled { return .failed(BackgroundLoad.harnesses.failureText) }
    return .loading("Loading harnesses…")
  }

  @ViewBuilder private func failureRow(
    _ load: BackgroundLoad, retry: @escaping () -> Void
  ) -> some View {
    if client.connected, let reason = client.loadFailure(load) {
      HarnessLoadStateRow(state: .failed(load.failureText), detail: reason, retry: retry)
    }
  }
}

extension View {
  /// The raised strip every row of the Harnesses list sits on.
  func preferenceRowSurface() -> some View {
    padding(.horizontal, MacTheme.Space.x16)
      .frame(height: 58)
      .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
  }
}
