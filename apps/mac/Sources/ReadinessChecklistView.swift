import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The setup checklist: what this machine has of what Soyeht needs, and the command that
/// installs each thing it lacks. The empty workspace and Preferences both show it.
struct ReadinessChecklistView: View {
  let client: EngineClient
  @Environment(EngineServiceSetup.self) private var service

  var body: some View {
    let checklist = client.readinessChecklist
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      HStack(spacing: MacTheme.Space.x10) {
        Text(checklist.headline)
          .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          .accessibilityIdentifier("readiness-headline")
        Spacer()
        Button("Check again") { client.refreshReadiness() }
          .buttonStyle(.paneSecondary)
          .accessibilityIdentifier("readiness-refresh")
      }
      group(checklist.rows.filter { $0.kind != .harness })
      if !client.connected, service.offersStart {
        EngineStartRow(setup: service)
      }
      let agents = checklist.rows(of: .harness)
      if !agents.isEmpty {
        Text("Agents · one is enough")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
          .foregroundStyle(MacTheme.secondary)
          .padding(.top, MacTheme.Space.x6)
        group(agents)
      }
      if let failure = client.loadFailure(.readiness) {
        Text(failure)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      }
    }
    .task(id: client.connected) {
      if client.connected { client.refreshReadiness() }
    }
  }

  private func group(_ rows: [ReadinessChecklist.Row]) -> some View {
    VStack(spacing: MacTheme.Space.x4) {
      ForEach(rows) { row in ReadinessRowView(row: row) }
    }
  }
}

extension View {
  /// The well every row of the setup list sits in.
  fileprivate func readinessRow(identifier: String) -> some View {
    padding(.horizontal, MacTheme.Space.x10)
      .frame(minHeight: DesignTokens.Layout.readinessRowHeight)
      .paneWell(radius: MacTheme.Radius.compact)
      .accessibilityIdentifier(identifier)
  }
}

private struct ReadinessRowView: View {
  let row: ReadinessChecklist.Row

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Image(systemName: symbol)
        .foregroundStyle(color)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
        Text(row.title)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        if let note = row.note {
          Text(note)
            .font(MacTheme.chrome(MacTheme.TypeScale.micro))
            .foregroundStyle(MacTheme.muted)
            .textSelection(.enabled)
        }
      }
      if !row.required && row.kind == .tool {
        Text("optional")
          .font(MacTheme.chrome(MacTheme.TypeScale.micro))
          .foregroundStyle(MacTheme.muted)
      }
      Spacer(minLength: MacTheme.Space.x10)
      if let hint = row.hint {
        Text(hint)
          .font(MacTheme.mono(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
          .textSelection(.enabled)
          .lineLimit(1)
        Button {
          NSPasteboard.general.clearContents()
          NSPasteboard.general.setString(hint, forType: .string)
        } label: {
          Image(systemName: "doc.on.doc")
        }
        .buttonStyle(.plain)
        .help("Copy the command")
        .accessibilityLabel("Copy the install command for \(row.title)")
      }
    }
    .readinessRow(identifier: "readiness-row-\(row.id)")
  }

  private var symbol: String {
    switch (row.state, row.required || row.kind == .harness) {
    case (.ready, _): "checkmark.circle.fill"
    case (.missing, true): "exclamationmark.triangle.fill"
    case (.missing, false): "circle"
    }
  }

  private var color: Color {
    switch (row.state, row.required || row.kind == .harness) {
    case (.ready, _): MacTheme.success
    case (.missing, true): MacTheme.warning
    case (.missing, false): MacTheme.muted
    }
  }
}

/// The way to start the engine from the app's own bundle, for a person who has not turned on its
/// background start (or whose start failed). It is a button, never an automatic registration.
private struct EngineStartRow: View {
  let setup: EngineServiceSetup

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Text(message)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Spacer(minLength: MacTheme.Space.x10)
      if setup.phase != .moveFirst {
        Button(buttonTitle) { Task { await setup.enable() } }
          .buttonStyle(.paneSecondary)
          .accessibilityIdentifier("engine-service-start")
      }
    }
    .readinessRow(identifier: "readiness-engine-start")
  }

  private var message: String {
    switch setup.phase {
    case .moveFirst:
      "Move Soyeht Next to your Applications folder and open it from there to start the engine."
    case .failed(let detail): detail
    case .ready: "Soyeht's engine is set up to start at login, but it is not running now."
    default: "Start the engine now and keep it running whenever you are logged in."
    }
  }

  private var buttonTitle: String {
    if case .failed = setup.phase { "Try again" } else { "Start Soyeht" }
  }
}
