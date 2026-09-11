import AppKit
import CoreImage.CIFilterBuiltins
import SoyehtClient
import SoyehtProtocol
import SwiftUI

private enum Palette {
  static let canvas = Color(red: 224 / 255, green: 229 / 255, blue: 236 / 255)
  static let face = Color(red: 232 / 255, green: 237 / 255, blue: 244 / 255)
  static let ink = Color(red: 62 / 255, green: 74 / 255, blue: 102 / 255)
  static let secondary = Color(red: 110 / 255, green: 122 / 255, blue: 150 / 255)
  static let accent = Color(red: 91 / 255, green: 124 / 255, blue: 250 / 255)
  static let shadow = Color(red: 166 / 255, green: 180 / 255, blue: 200 / 255)
}
private struct Raised: ViewModifier {
  var radius: CGFloat = 16
  func body(content: Content) -> some View {
    content.background(Palette.face, in: RoundedRectangle(cornerRadius: radius))
      .shadow(color: .white.opacity(0.85), radius: 8, x: -5, y: -5)
      .shadow(color: Palette.shadow.opacity(0.55), radius: 8, x: 5, y: 5)
  }
}
struct WorkspaceWindow: View {
  @Bindable var client: EngineClient
  @State private var paneToClose: String?
  @State private var showNewPane = false
  @State private var showPairing = false
  @State private var deviceToRevoke: String?
  var body: some View {
    VStack(spacing: 22) {
      HStack(spacing: 16) {
        Text("Soyeht").font(.system(size: 22, weight: .bold, design: .rounded))
        Text("DEV").font(.system(size: 10, weight: .bold, design: .rounded))
          .padding(.horizontal, 9).padding(.vertical, 5)
          .background(Palette.accent.opacity(0.12), in: Capsule()).foregroundStyle(Palette.accent)
        Spacer()
        Label(
          client.connected ? "Connected" : "Reconnecting",
          systemImage: client.connected ? "circle.fill" : "arrow.trianglehead.2.clockwise"
        )
        .font(.system(size: 12)).foregroundStyle(Palette.secondary)
        Button {
          client.beginPairing()
          showPairing = true
        } label: {
          Label("Add iPhone", systemImage: "iphone")
            .padding(.horizontal, 16).padding(.vertical, 10).modifier(Raised(radius: 12))
        }.disabled(!client.connected)
      }.padding(.leading, 72).padding(.top, 3)
      HStack(alignment: .top, spacing: 22) {
        VStack(alignment: .leading, spacing: 16) {
          HStack {
            Text("WORKSPACES").font(.system(size: 10, weight: .bold)).tracking(1.4)
            Spacer()
            Button(action: presentNewPane) { Image(systemName: "plus").padding(6) }
              .help("Open a terminal or agent pane").disabled(!client.connected)
          }.foregroundStyle(Palette.secondary)
          ScrollView {
            VStack(alignment: .leading, spacing: 20) {
              ForEach(client.workspaces, id: \.workspaceId) { workspace in
                VStack(alignment: .leading, spacing: 9) {
                  Label(workspace.name, systemImage: "folder")
                    .font(.system(size: 14, weight: .semibold, design: .rounded)).lineLimit(1)
                  ForEach(
                    client.panes.filter { $0.workspaceId == workspace.workspaceId }, id: \.paneId
                  ) { pane in
                    Button {
                      client.select(pane.paneId)
                    } label: {
                      HStack(spacing: 9) {
                        Image(
                          systemName: pane.mode == .agent
                            ? "bubble.left.and.bubble.right" : "terminal")
                        VStack(alignment: .leading, spacing: 3) {
                          Text(pane.name).font(.system(size: 13, weight: .semibold))
                          Text(pane.phrase).font(.system(size: 10)).foregroundStyle(
                            Palette.secondary)
                        }.lineLimit(1)
                        Spacer(minLength: 0)
                      }.padding(11).frame(maxWidth: .infinity, alignment: .leading)
                        .background(
                          client.selectedPaneId == pane.paneId
                            ? Palette.accent.opacity(0.12) : .clear,
                          in: RoundedRectangle(cornerRadius: 12))
                    }.contextMenu {
                      Button("Close pane", role: .destructive) { paneToClose = pane.paneId }
                    }
                  }
                }
              }
            }
          }
          Spacer(minLength: 0)
          Button(action: presentNewPane) {
            Label("New pane", systemImage: "plus")
              .font(.system(size: 13, weight: .semibold)).frame(maxWidth: .infinity)
              .padding(.vertical, 13).modifier(Raised(radius: 12))
          }.disabled(!client.connected)
        }.frame(width: 218).padding(.vertical, 10)
        VStack(alignment: .leading, spacing: 0) {
          if let pane = client.selectedPane {
            HStack {
              Image(
                systemName: pane.mode == .agent ? "bubble.left.and.bubble.right" : "terminal"
              ).foregroundStyle(Palette.accent)
              Text(pane.name).font(.system(size: 15, weight: .semibold, design: .rounded))
              if pane.mode == .agent, let harness = pane.agent?.harness {
                Text(harness).font(.system(size: 11)).foregroundStyle(Palette.secondary)
              } else if pane.generation > 1 {
                Text("new shell").font(.system(size: 11)).foregroundStyle(Palette.secondary)
              }
              Spacer()
              Text(pane.phrase).font(.system(size: 12)).foregroundStyle(Palette.secondary)
              Button {
                paneToClose = pane.paneId
              } label: {
                Image(systemName: "xmark").padding(6)
              }
              .help("Close this pane").disabled(!client.connected)
            }.padding(.horizontal, 20).padding(.vertical, 16)
            Text(pane.cwd).font(.system(size: 11, design: .monospaced)).foregroundStyle(
              Palette.secondary
            )
            .lineLimit(1).truncationMode(.middle).padding(.horizontal, 20).padding(.bottom, 12)
            Rectangle().fill(Palette.canvas).frame(height: 1)
            if pane.mode == .agent {
              AgentConversationView(client: client, paneId: pane.paneId)
            } else {
              MacTerminal(client: client).padding(14)
              if let gap = client.gapText {
                Label(gap, systemImage: "exclamationmark.triangle")
                  .font(.system(size: 12)).foregroundStyle(.orange).padding(12)
              }
            }
          } else {
            Spacer()
            VStack(spacing: 18) {
              Image(systemName: "rectangle.split.2x1").font(.system(size: 42, weight: .light))
                .foregroundStyle(Palette.secondary)
              Text("Your space to begin").font(
                .system(size: 23, weight: .semibold, design: .rounded))
              Text(
                "Open a terminal or agent in a directory. The process continues\neven when you close this window."
              )
              .font(.system(size: 14)).foregroundStyle(Palette.secondary).multilineTextAlignment(
                .center)
              Button("New pane", action: presentNewPane)
                .padding(.horizontal, 22).padding(.vertical, 12).modifier(Raised(radius: 12))
                .disabled(!client.connected)
            }.frame(maxWidth: .infinity)
            Spacer()
          }
        }.frame(maxWidth: .infinity, maxHeight: .infinity).modifier(Raised(radius: 20))
      }
      HStack {
        Text(client.connectionText).font(.system(size: 11))
        Spacer()
        Text("Soyeht · Dev").font(.system(size: 11, weight: .medium, design: .rounded))
      }.foregroundStyle(Palette.secondary)
    }.padding(24).background(Palette.canvas).foregroundStyle(Palette.ink).buttonStyle(.plain)
      .sheet(isPresented: $showNewPane) {
        NewPaneSheet(client: client, isPresented: $showNewPane)
      }
      .sheet(isPresented: $showPairing, onDismiss: { client.clearPairCode() }) { pairing }
      .alert(
        "Close pane?",
        isPresented: Binding(get: { paneToClose != nil }, set: { if !$0 { paneToClose = nil } })
      ) {
        Button("Cancel", role: .cancel) { paneToClose = nil }
        Button("Close", role: .destructive) {
          if let id = paneToClose { client.closePane(id) }
          paneToClose = nil
        }
      } message: {
        Text("The process in this pane will be terminated.")
      }
      .alert(
        "Could not complete",
        isPresented: Binding(
          get: { client.problem != nil }, set: { if !$0 { client.problem = nil } })
      ) {
        Button("OK") { client.problem = nil }
      } message: {
        Text(client.problem ?? "")
      }
      .overlay(alignment: .bottom) {
        if let request = client.pairRequests.first {
          VStack(alignment: .leading, spacing: 12) {
            Text("Allow access for \(request.deviceName)?").font(.headline)
            Text("Compare this fingerprint on the iPhone:").font(.subheadline)
            Text(request.fingerprint).font(.system(size: 12, design: .monospaced)).textSelection(
              .enabled)
            HStack {
              Button("Deny") { client.decidePairing(request, approve: false) }
              Spacer()
              Button("Allow") { client.decidePairing(request, approve: true) }.buttonStyle(
                .borderedProminent)
            }
          }.padding(22).frame(width: 420).modifier(Raised(radius: 20)).padding(.bottom, 40)
        }
      }
  }
  private func presentNewPane() {
    client.refreshHarnesses()
    showNewPane = true
  }
  private var pairing: some View {
    ScrollView { pairingContent }.frame(width: 520, height: 720).background(Palette.canvas)
  }
  private var pairingContent: some View {
    VStack(spacing: 20) {
      Text("Continue on iPhone").font(.system(size: 24, weight: .semibold, design: .rounded))
      Text(
        "Open Soyeht Dev on iPhone and scan this code.\nBoth devices must be on the same tailnet."
      )
      .multilineTextAlignment(.center).foregroundStyle(Palette.secondary)
      if let code = client.pairCode, let image = qr(code.uri) {
        Image(nsImage: image).interpolation(.none).resizable().frame(width: 260, height: 260)
          .padding(16).background(.white, in: RoundedRectangle(cornerRadius: 16))
        Text("Code valid for 10 minutes.").font(.caption).foregroundStyle(Palette.secondary)
      } else {
        ProgressView().frame(width: 292, height: 292)
      }
      if let request = client.pairRequests.first {
        Text(request.deviceName).font(.headline)
        Text(request.fingerprint).font(.system(size: 11, design: .monospaced)).textSelection(
          .enabled)
        HStack {
          Button("Deny") { client.decidePairing(request, approve: false) }
          Button("Allow") {
            client.decidePairing(request, approve: true)
            showPairing = false
          }
          .buttonStyle(.borderedProminent)
        }
      }
      if !client.devices.isEmpty {
        VStack(alignment: .leading, spacing: 12) {
          Text("Paired iPhones").font(.headline)
          ForEach(client.devices, id: \.deviceId) { device in
            HStack {
              Label(device.name, systemImage: "iphone")
              Spacer()
              Button("Revoke", role: .destructive) { deviceToRevoke = device.deviceId }
            }
          }
        }.padding(.vertical, 12)
      }
      Button("Close") { showPairing = false }
    }.padding(32).frame(width: 460).background(Palette.canvas).foregroundStyle(Palette.ink)
      .confirmationDialog(
        "Revoke this iPhone's access?",
        isPresented: Binding(
          get: { deviceToRevoke != nil }, set: { if !$0 { deviceToRevoke = nil } })
      ) {
        Button("Revoke", role: .destructive) {
          if let id = deviceToRevoke { client.revokeDevice(id) }
          deviceToRevoke = nil
        }
        Button("Cancel", role: .cancel) { deviceToRevoke = nil }
      } message: {
        Text("Connections will close. Access will require pairing again.")
      }
  }
  private func qr(_ value: String) -> NSImage? {
    let filter = CIFilter.qrCodeGenerator()
    filter.message = Data(value.utf8)
    guard let image = filter.outputImage,
      let cg = CIContext().createCGImage(image, from: image.extent)
    else { return nil }
    return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
  }
}

private struct NewPaneSheet: View {
  @Bindable var client: EngineClient
  @Binding var isPresented: Bool
  @State private var mode = PaneMode.terminal
  @State private var directory: URL?
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var permission = PermissionMode.ask

  private var availableHarnesses: [HarnessInfo] { client.harnesses.filter(\.available) }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? {
    catalogue?.options.first { $0.category == "thought_level" || $0.id == "effort" }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 20) {
      Text("New pane").font(.system(size: 24, weight: .semibold, design: .rounded))
      Picker("Pane type", selection: $mode) {
        Text("Terminal").tag(PaneMode.terminal)
        Text("Agent").tag(PaneMode.agent)
      }.pickerStyle(.segmented)
      VStack(alignment: .leading, spacing: 8) {
        Text("Directory").font(.caption.weight(.semibold)).foregroundStyle(Palette.secondary)
        HStack {
          Text(directory?.path ?? "Choose where this pane will work")
            .font(.system(size: 12, design: .monospaced)).lineLimit(1).truncationMode(.middle)
          Spacer()
          Button("Choose…", action: chooseDirectory).buttonStyle(.bordered)
        }.padding(12).modifier(Raised(radius: 12))
      }
      if mode == .agent {
        agentOptions
      }
      Spacer(minLength: 0)
      HStack {
        Button("Cancel", role: .cancel) { isPresented = false }
        Spacer()
        Button("Create", action: create).buttonStyle(.borderedProminent)
          .disabled(directory == nil || (mode == .agent && harness.isEmpty))
      }
    }.padding(28).frame(width: 520).frame(minHeight: mode == .agent ? 560 : 330)
      .background(Palette.canvas).foregroundStyle(Palette.ink)
      .onAppear {
        client.refreshHarnesses()
        selectFirstHarness()
      }
      .onChange(of: client.harnesses.map(\.id)) { _, _ in selectFirstHarness() }
      .onChange(of: harness) { _, value in
        model = ""
        effort = ""
        if !value.isEmpty { client.refreshCatalogue(harness: value) }
      }
  }

  private var agentOptions: some View {
    VStack(alignment: .leading, spacing: 14) {
      if availableHarnesses.isEmpty {
        ContentUnavailableView(
          "No available agents", systemImage: "person.crop.circle.badge.exclamationmark",
          description: Text("Install or configure an ACP agent, then reopen this sheet."))
      } else {
        Picker("Agent", selection: $harness) {
          ForEach(availableHarnesses, id: \.id) { item in Text(item.name).tag(item.id) }
        }
        Picker("Model", selection: $model) {
          Text("Agent default").tag("")
          ForEach(catalogue?.models ?? [], id: \.id) { item in Text(item.name).tag(item.id) }
        }
        Picker("Effort", selection: $effort) {
          Text("Agent default").tag("")
          ForEach(effortOption?.options ?? [], id: \.value) { choice in
            Text(choice.name).tag(choice.value)
          }
        }
        Picker("Permission", selection: $permission) {
          Text("Ask every time").tag(PermissionMode.ask)
          Text("Accept edits in this directory").tag(PermissionMode.accept_edits)
          Text("Allow all agent actions").tag(PermissionMode.allow_all)
        }
        if catalogue?.source == "not_probed",
          catalogue?.models.isEmpty != false
        {
          Text("The agent will use its default and report available models after opening.")
            .font(.caption).foregroundStyle(Palette.secondary)
        }
      }
    }.pickerStyle(.menu)
  }

  private func selectFirstHarness() {
    guard !availableHarnesses.contains(where: { $0.id == harness }),
      let first = availableHarnesses.first
    else { return }
    harness = first.id
  }

  private func chooseDirectory() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.prompt = "Choose"
    panel.begin { response in
      if response == .OK { directory = panel.url }
    }
  }

  private func create() {
    guard let directory else { return }
    if mode == .terminal {
      client.createPane(directory: directory)
    } else {
      client.createAgentPane(
        directory: directory, harness: harness, model: model.isEmpty ? nil : model,
        effort: effort.isEmpty ? nil : effort, permission: permission)
    }
    isPresented = false
  }
}
