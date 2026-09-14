import AVFoundation
import SoyehtClient
import SoyehtProtocol
import SwiftUI
import VisionKit

@MainActor enum PhonePalette {
  static var canvas: Color { SoyehtThemeColors.canvas }
  static var face: Color { SoyehtThemeColors.face }
  static var ink: Color { SoyehtThemeColors.ink }
  static var secondary: Color { SoyehtThemeColors.secondary }
  static var accent: Color { SoyehtThemeColors.accent }
  static var success: Color { SoyehtThemeColors.success }
  static var warning: Color { SoyehtThemeColors.warning }
  static var danger: Color { SoyehtThemeColors.danger }
}
struct PhoneHome: View {
  @Bindable var model: PhoneModel
  @State private var scanning = false
  @State private var uri = ""
  @State private var forget = false
  @State private var showingSettings = false
  var body: some View {
    NavigationStack {
      Group {
        if let pairing = model.pairing {
          pairingView(pairing)
        } else if let client = model.client {
          if client.connected {
            paneList(client)
          } else {
            VStack(spacing: 20) {
              if client.credentialRejected {
                Image(systemName: "lock.iphone").font(.largeTitle)
                Text("Pair again on the Mac").font(.headline)
              } else {
                ProgressView()
                Text("Fetching Mac state…").font(.headline)
              }
              Text(client.connectionText).font(.subheadline).multilineTextAlignment(.center)
                .foregroundStyle(PhonePalette.secondary)
              Text("Keep Tailscale connected on both devices.")
                .font(.footnote).multilineTextAlignment(.center)
              Button("Forget this Mac") { forget = true }
            }.padding(28)
          }
        } else {
          onboarding
        }
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(PhonePalette.canvas).foregroundStyle(PhonePalette.ink)
      .navigationTitle("Soyeht · Dev")
      .toolbar {
        if model.client != nil {
          ToolbarItem(placement: .topBarTrailing) {
            Menu {
              Button {
                showingSettings = true
              } label: {
                Label("Settings", systemImage: "gearshape")
              }
              Button("Forget this Mac", role: .destructive) { forget = true }
            } label: {
              Image(systemName: "ellipsis.circle")
            }
            .accessibilityLabel("Mac options")
          }
        }
      }
    }
    .tint(PhonePalette.accent)
    .sheet(isPresented: $showingSettings) {
      if let client = model.client {
        PhoneSettings(client: client)
      }
    }
    .sheet(isPresented: $scanning) {
      QRScanner { value in
        scanning = false
        model.pair(value)
      } failure: { message in
        scanning = false
        model.problem = message
      }
      .overlay(alignment: .topTrailing) {
        Button("Close") { scanning = false }.buttonStyle(.borderedProminent).padding(24)
      }
    }
    .alert(
      "Could not complete",
      isPresented: Binding(get: { model.problem != nil }, set: { if !$0 { model.problem = nil } })
    ) {
      Button("OK") { model.problem = nil }
    } message: {
      Text(model.problem ?? "")
    }
    .confirmationDialog("Forget this Mac?", isPresented: $forget, titleVisibility: .visible) {
      Button("Forget", role: .destructive) { model.forgetMac() }
      Button("Cancel", role: .cancel) {}
    } message: {
      Text(
        "The credential will be removed from this iPhone. To revoke access at the engine, use the Mac."
      )
    }
  }
  private var onboarding: some View {
    ScrollView {
      VStack(spacing: 26) {
        Image(systemName: "laptopcomputer.and.iphone").font(.system(size: 64, weight: .light))
          .foregroundStyle(PhonePalette.accent).padding(.top, 38)
        Text("Your work, with you").font(.system(size: 28, weight: .bold, design: .rounded))
          .multilineTextAlignment(.center)
        Text(
          "On your Mac, open Soyeht Dev and choose “Add iPhone”. Then scan the QR and confirm access on the Mac."
        )
        .multilineTextAlignment(.center).foregroundStyle(PhonePalette.secondary)
        Button {
          scan()
        } label: {
          Label("Scan Mac QR", systemImage: "qrcode.viewfinder")
            .font(.headline).foregroundStyle(PhonePalette.face).frame(maxWidth: .infinity).padding(
              16)
        }.buttonStyle(.borderedProminent)
        DisclosureGroup("Use a pairing link") {
          VStack(spacing: 12) {
            TextField("soyeht-next://pair…", text: $uri, axis: .vertical)
              .textInputAutocapitalization(.never).autocorrectionDisabled()
              .textFieldStyle(.roundedBorder).accessibilityIdentifier("pairing-uri")
            Button("Connect") {
              let value = uri
              uri = ""
              model.pair(value)
            }
            .disabled(uri.isEmpty)
          }.padding(.top, 10)
        }
        Text("Terminal screens are read-only. You can message agents and answer permissions.")
          .font(.footnote).foregroundStyle(PhonePalette.secondary).multilineTextAlignment(.center)
      }.padding(28)
    }
  }
  private func paneList(_ client: EngineClient) -> some View {
    List {
      if client.panes.isEmpty {
        ContentUnavailableView(
          "No open panes", systemImage: "rectangle.split.2x1",
          description: Text("Open a pane on the Mac to continue here.")
        )
        .listRowBackground(Color.clear)
      }
      ForEach(client.workspaces, id: \.workspaceId) { workspace in
        Section(workspace.name) {
          ForEach(client.panes.filter { $0.workspaceId == workspace.workspaceId }, id: \.paneId) {
            pane in
            NavigationLink {
              PhonePane(client: client, paneId: pane.paneId)
            } label: {
              Label {
                VStack(alignment: .leading, spacing: 5) {
                  Text(pane.name).font(.system(.headline, design: .rounded))
                  Text(pane.phrase).font(.caption).foregroundStyle(PhonePalette.secondary)
                }.padding(.vertical, 6)
              } icon: {
                Image(systemName: paneIcon(pane.mode)).foregroundStyle(PhonePalette.accent)
              }
            }.listRowBackground(PhonePalette.face)
          }
        }
      }
    }.scrollContentBackground(.hidden)
  }
  private func pairingView(_ session: PairingSession) -> some View {
    VStack(spacing: 22) {
      ProgressView()
      Text("Confirm on the Mac").font(.system(.title2, design: .rounded, weight: .bold))
      Text(session.status).multilineTextAlignment(.center)
      Text(session.fingerprint).font(.system(size: 14, design: .monospaced))
        .textSelection(.enabled).padding(18).background(
          PhonePalette.face, in: RoundedRectangle(cornerRadius: 16))
      Button("Cancel", role: .cancel) { model.cancelPairing() }
    }.padding(28)
  }

  private func paneIcon(_ mode: PaneMode) -> String {
    switch mode {
    case .terminal: "terminal"
    case .agent: "bubble.left.and.bubble.right"
    case .editor: "doc.text"
    case .web: "globe"
    case .writing: "text.book.closed"
    }
  }
  private func scan() {
    Task {
      guard DataScannerViewController.isSupported else {
        model.problem =
          "The scanner is unavailable on this device. Use the pairing link."
        return
      }
      let allowed = await AVCaptureDevice.requestAccess(for: .video)
      guard allowed, DataScannerViewController.isAvailable else {
        model.problem = "Allow camera access in Settings or use the pairing link."
        return
      }
      scanning = true
    }
  }
}

private struct PhonePane: View {
  let client: EngineClient
  let paneId: String
  var body: some View {
    VStack(spacing: 0) {
      if client.connected, let pane = client.panes.first(where: { $0.paneId == paneId }) {
        if pane.mode == .web {
          PhoneWebPane(client: client, pane: pane)
        } else if pane.mode == .agent {
          AgentConversationView(client: client, paneId: paneId)
        } else {
          HStack {
            Text(pane.phrase)
            if pane.generation > 1 { Text("· new shell") }
            Spacer()
            Image(systemName: "eye").accessibilityLabel("Read-only")
          }.font(.caption).padding(12).foregroundStyle(PhonePalette.secondary)
          PhoneTerminal(client: client)
          if let gap = client.gapText {
            Label(gap, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(
              PhonePalette.warning
            ).padding(12)
          }
        }
      } else {
        ContentUnavailableView(
          "Updating screen", systemImage: "arrow.trianglehead.2.clockwise",
          description: Text(client.connectionText))
      }
    }.background(PhonePalette.face)
      .navigationTitle(client.panes.first(where: { $0.paneId == paneId })?.name ?? "Pane")
      .navigationBarTitleDisplayMode(.inline)
      .onAppear { client.select(paneId) }
  }
}

private struct PhoneWebPane: View {
  let client: EngineClient
  let pane: Pane

  var body: some View {
    VStack(spacing: 20) {
      Spacer()
      Image(systemName: "macbook.and.iphone")
        .font(.system(size: 42, weight: .light))
        .foregroundStyle(PhonePalette.accent)
      VStack(spacing: 7) {
        Text(pane.title?.isEmpty == false ? pane.title ?? pane.name : pane.name)
          .font(.system(.title3, design: .rounded, weight: .bold))
          .multilineTextAlignment(.center)
        Text(pane.url ?? "No address")
          .font(.system(.footnote, design: .monospaced))
          .foregroundStyle(PhonePalette.secondary)
          .multilineTextAlignment(.center)
          .textSelection(.enabled)
      }
      Button {
        openOnMac()
      } label: {
        Label("Open on the Mac", systemImage: "macbook")
          .font(.system(.headline, design: .rounded))
          .frame(maxWidth: 260)
      }
      .buttonStyle(.borderedProminent)
      Text("The page stays on your Mac. This iPhone shows its title and address only.")
        .font(.footnote)
        .foregroundStyle(PhonePalette.secondary)
        .multilineTextAlignment(.center)
        .frame(maxWidth: 300)
      Spacer()
    }
    .padding(28)
  }

  private func openOnMac() {
    guard let url = pane.url else { return }
    Task {
      do {
        _ = try await client.navigateWeb(paneId: pane.paneId, url: url)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}
