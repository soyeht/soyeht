import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WebWorkspace: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String?
  let viewportSize: CGSize
  let onClose: (String) -> Void
  let onNewPane: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onTasks: () -> Void

  private var panes: [Pane] {
    guard let workspaceId else { return [] }
    return client.panes.filter { $0.workspaceId == workspaceId && $0.mode == .web }
  }

  var body: some View {
    if panes.isEmpty {
      WebEmptyState(client: client, workspaceId: workspaceId)
        .padding(EdgeInsets(top: 8, leading: 6, bottom: 6, trailing: 6))
    } else {
      PaneGrid(
        client: client, webHost: webHost, workspaceId: workspaceId, mode: .web,
        viewportSize: viewportSize, onClose: onClose, onNewPane: onNewPane, onSplit: onSplit,
        onTasks: onTasks)
    }
  }
}

private struct WebEmptyState: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  @State private var address = ""
  @State private var incognito = false

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      Text("Open a local server")
        .font(MacTheme.ui(MacTheme.TypeScale.subheading, weight: .bold))
      Text("Choose a server detected on this Mac, or enter an address.")
        .font(MacTheme.ui(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
      serverList
      addressField
      Toggle(isOn: $incognito) {
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text("Incognito pane")
            .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
          Text("Use a temporary session that is discarded when this pane closes.")
            .font(MacTheme.ui(MacTheme.TypeScale.micro))
            .foregroundStyle(MacTheme.secondary)
        }
      }
      .toggleStyle(.switch)
    }
    .padding(MacTheme.Space.x14)
    .frame(width: 420)
    .paneShell()
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .task { await refreshWhileVisible() }
  }

  @ViewBuilder private var serverList: some View {
    if client.localServers.isEmpty {
      HStack(spacing: MacTheme.Space.x10) {
        ProgressView().controlSize(.small)
        Text("Looking for local servers…")
          .font(MacTheme.ui(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.muted)
      }
      .padding(.horizontal, MacTheme.Space.x10)
      .frame(maxWidth: .infinity, minHeight: 46, alignment: .leading)
      .paneWell(radius: MacTheme.Radius.compact)
    } else {
      ScrollView {
        LazyVStack(spacing: MacTheme.Space.x8) {
          ForEach(client.localServers, id: \.port) { server in
            Button {
              open("http://127.0.0.1:\(server.port)")
            } label: {
              LocalServerRow(server: server)
            }
            .buttonStyle(.plain)
          }
        }
      }
      .frame(maxHeight: 280)
    }
  }

  private var addressField: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "globe")
        .font(.system(size: 11, weight: .medium))
        .foregroundStyle(MacTheme.secondary)
      TextField("localhost:3000 or example.com", text: $address)
        .textFieldStyle(.plain)
        .font(MacTheme.ui(MacTheme.TypeScale.caption))
        .onSubmit { open(address) }
    }
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: 28)
    .paneWell(radius: MacTheme.Radius.pill)
  }

  private func refreshWhileVisible() async {
    client.refreshLocalServers()
    while !Task.isCancelled {
      do { try await Task.sleep(for: .seconds(5)) } catch { return }
      client.refreshLocalServers()
    }
  }

  private func open(_ url: String) {
    guard let workspaceId, !url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      return
    }
    Task {
      do {
        _ = try await client.openWeb(workspaceId: workspaceId, url: url, incognito: incognito)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}

private struct LocalServerRow: View {
  let server: LocalServer

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
        Text(server.title ?? server.command)
          .font(MacTheme.mono(MacTheme.TypeScale.caption, weight: .bold))
          .lineLimit(1)
        Text("\(server.cwd) · port \(identifier: server.port) · pid \(identifier: server.pid)")
          .font(MacTheme.ui(MacTheme.TypeScale.micro))
          .foregroundStyle(MacTheme.secondary)
          .lineLimit(1)
      }
      Spacer(minLength: 0)
      Image(systemName: "arrow.right")
        .font(.system(size: 12, weight: .semibold))
        .foregroundStyle(MacTheme.accent)
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(maxWidth: .infinity, minHeight: 46, alignment: .leading)
    .paneWell(radius: MacTheme.Radius.compact)
  }
}
