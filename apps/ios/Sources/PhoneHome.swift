import AVFoundation
import SoyehtClient
import SwiftUI
import VisionKit

private enum PhonePalette {
  static let canvas = Color(red: 224 / 255, green: 229 / 255, blue: 236 / 255)
  static let face = Color(red: 232 / 255, green: 237 / 255, blue: 244 / 255)
  static let ink = Color(red: 62 / 255, green: 74 / 255, blue: 102 / 255)
  static let secondary = Color(red: 110 / 255, green: 122 / 255, blue: 150 / 255)
  static let accent = Color(red: 91 / 255, green: 124 / 255, blue: 250 / 255)
}
struct PhoneHome: View {
  @Bindable var model: PhoneModel
  @State private var scanning = false
  @State private var uri = ""
  @State private var forget = false
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
                Text("Pareie novamente no Mac").font(.headline)
              } else {
                ProgressView()
                Text("Buscando o estado do Mac…").font(.headline)
              }
              Text(client.connectionText).font(.subheadline).multilineTextAlignment(.center)
                .foregroundStyle(PhonePalette.secondary)
              Text("Mantenha o Tailscale conectado nos dois dispositivos.")
                .font(.footnote).multilineTextAlignment(.center)
              Button("Esquecer este Mac") { forget = true }
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
              Button("Esquecer este Mac", role: .destructive) { forget = true }
            } label: {
              Image(systemName: "ellipsis.circle")
            }
            .accessibilityLabel("Opções do Mac")
          }
        }
      }
    }
    .tint(PhonePalette.accent)
    .sheet(isPresented: $scanning) {
      QRScanner { value in
        scanning = false
        model.pair(value)
      } failure: { message in
        scanning = false
        model.problem = message
      }
      .overlay(alignment: .topTrailing) {
        Button("Fechar") { scanning = false }.buttonStyle(.borderedProminent).padding(24)
      }
    }
    .alert(
      "Não foi possível concluir",
      isPresented: Binding(get: { model.problem != nil }, set: { if !$0 { model.problem = nil } })
    ) {
      Button("OK") { model.problem = nil }
    } message: {
      Text(model.problem ?? "")
    }
    .confirmationDialog("Esquecer este Mac?", isPresented: $forget, titleVisibility: .visible) {
      Button("Esquecer", role: .destructive) { model.forgetMac() }
      Button("Cancelar", role: .cancel) {}
    } message: {
      Text("A credencial será removida deste iPhone. Para revogar o acesso no engine, use o Mac.")
    }
  }
  private var onboarding: some View {
    ScrollView {
      VStack(spacing: 26) {
        Image(systemName: "laptopcomputer.and.iphone").font(.system(size: 64, weight: .light))
          .foregroundStyle(PhonePalette.accent).padding(.top, 38)
        Text("Seu terminal, com você").font(.system(size: 28, weight: .bold, design: .rounded))
          .multilineTextAlignment(.center)
        Text(
          "No Mac, abra o Soyeht Dev e escolha “Adicionar iPhone”. Depois, leia o QR e confirme o acesso no Mac."
        )
        .multilineTextAlignment(.center).foregroundStyle(PhonePalette.secondary)
        Button {
          scan()
        } label: {
          Label("Ler QR do Mac", systemImage: "qrcode.viewfinder")
            .font(.headline).foregroundStyle(.white).frame(maxWidth: .infinity).padding(16)
        }.buttonStyle(.borderedProminent)
        DisclosureGroup("Usar um link de pareamento") {
          VStack(spacing: 12) {
            TextField("soyeht-next://pair…", text: $uri, axis: .vertical)
              .textInputAutocapitalization(.never).autocorrectionDisabled()
              .textFieldStyle(.roundedBorder).accessibilityIdentifier("pairing-uri")
            Button("Conectar") {
              let value = uri
              uri = ""
              model.pair(value)
            }
            .disabled(uri.isEmpty)
          }.padding(.top, 10)
        }
        Text("A tela é somente leitura. O shell continua no Mac.")
          .font(.footnote).foregroundStyle(PhonePalette.secondary).multilineTextAlignment(.center)
      }.padding(28)
    }
  }
  private func paneList(_ client: EngineClient) -> some View {
    List {
      if client.panes.isEmpty {
        ContentUnavailableView(
          "Nenhum terminal aberto", systemImage: "terminal",
          description: Text("Abra uma pane no Mac para continuar aqui.")
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
                Image(systemName: "terminal").foregroundStyle(PhonePalette.accent)
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
      Text("Confirme no Mac").font(.system(.title2, design: .rounded, weight: .bold))
      Text(session.status).multilineTextAlignment(.center)
      Text(session.fingerprint).font(.system(size: 14, design: .monospaced))
        .textSelection(.enabled).padding(18).background(
          PhonePalette.face, in: RoundedRectangle(cornerRadius: 16))
      Button("Cancelar", role: .cancel) { model.cancelPairing() }
    }.padding(28)
  }
  private func scan() {
    Task {
      guard DataScannerViewController.isSupported else {
        model.problem =
          "A câmera de leitura não está disponível neste dispositivo. Use o link de pareamento."
        return
      }
      let allowed = await AVCaptureDevice.requestAccess(for: .video)
      guard allowed, DataScannerViewController.isAvailable else {
        model.problem = "Permita o uso da câmera nos Ajustes ou use o link de pareamento."
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
        HStack {
          Text(pane.phrase)
          if pane.generation > 1 { Text("· shell novo") }
          Spacer()
          Image(systemName: "eye").accessibilityLabel("Somente leitura")
        }.font(.caption).padding(12).foregroundStyle(PhonePalette.secondary)
        PhoneTerminal(client: client)
        if let gap = client.gapText {
          Label(gap, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(
            .orange
          ).padding(12)
        }
      } else {
        ContentUnavailableView(
          "Atualizando a tela", systemImage: "arrow.trianglehead.2.clockwise",
          description: Text(client.connectionText))
      }
    }.background(PhonePalette.face)
      .navigationTitle(client.panes.first(where: { $0.paneId == paneId })?.name ?? "Terminal")
      .navigationBarTitleDisplayMode(.inline)
      .onAppear { client.select(paneId) }
  }
}
