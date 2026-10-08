import AVFoundation
import SoyehtClient
import SwiftUI
import VisionKit

@MainActor enum PhonePalette {
  static var canvas: Color { SoyehtThemeColors.canvas }
  static var face: Color { SoyehtThemeColors.face }
  static var ink: Color { SoyehtThemeColors.ink }
  static var secondary: Color { SoyehtThemeColors.secondary }
  static var muted: Color { SoyehtThemeColors.muted }
  static var accent: Color { SoyehtThemeColors.accent }
  static var onAccent: Color { SoyehtThemeColors.onAccent }
  static var presentationLetterbox: Color { SoyehtThemeColors.presentationLetterbox }
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
    root
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
          "The credential will be removed from this device. To revoke access at the engine, use the Mac."
        )
      }
  }
  /// Once a Mac is paired, its workspaces own the screen; before that, onboarding or the
  /// pending pairing does.
  @ViewBuilder private var root: some View {
    if model.pairing == nil, let client = model.client {
      PhoneWorkspaceSplit(client: client, requestedPaneId: $model.requestedPaneId) {
        connectionStatus(client)
      } options: {
        macOptions
      }
    } else {
      NavigationStack {
        Group {
          if let pairing = model.pairing { pairingView(pairing) } else { onboarding }
        }
        .phoneHomeColumn()
      }
    }
  }
  private func connectionStatus(_ client: EngineClient) -> some View {
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
  private var macOptions: some View {
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
  private var onboarding: some View {
    ScrollView {
      VStack(spacing: 26) {
        SoyehtMark().padding(.top, 38)
        Text("Your work, with you").font(.system(size: 28, weight: .bold, design: .rounded))
          .multilineTextAlignment(.center)
        Text(
          "On your Mac, open Soyeht Dev and choose “Pair iPhone”; the same QR pairs an iPad. Then scan it and confirm access on the Mac."
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
        Text(
          "This device controls your Mac: type in terminals, drive agents and answer permissions."
        )
        .font(.footnote).foregroundStyle(PhonePalette.secondary).multilineTextAlignment(.center)
      }.padding(28).frame(maxWidth: 560).frame(maxWidth: .infinity)
    }
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

extension View {
  /// The home column's frame, canvas and title, shared by onboarding and the pane sidebar.
  func phoneHomeColumn() -> some View {
    frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(PhonePalette.canvas).foregroundStyle(PhonePalette.ink)
      .navigationTitle("Soyeht · Dev")
  }
}
