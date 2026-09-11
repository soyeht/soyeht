import AppKit
import CoreImage.CIFilterBuiltins
import SoyehtClient
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
          client.connected ? "Conectado" : "Reconectando",
          systemImage: client.connected ? "circle.fill" : "arrow.trianglehead.2.clockwise"
        )
        .font(.system(size: 12)).foregroundStyle(Palette.secondary)
        Button {
          client.beginPairing()
          showPairing = true
        } label: {
          Label("Adicionar iPhone", systemImage: "iphone")
            .padding(.horizontal, 16).padding(.vertical, 10).modifier(Raised(radius: 12))
        }.disabled(!client.connected)
      }.padding(.leading, 72).padding(.top, 3)
      HStack(alignment: .top, spacing: 22) {
        VStack(alignment: .leading, spacing: 16) {
          HStack {
            Text("WORKSPACES").font(.system(size: 10, weight: .bold)).tracking(1.4)
            Spacer()
            Button(action: createPane) { Image(systemName: "plus").padding(6) }
              .help("Abrir um terminal em uma pasta").disabled(!client.connected)
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
                        Image(systemName: "terminal")
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
                      Button("Fechar terminal", role: .destructive) { paneToClose = pane.paneId }
                    }
                  }
                }
              }
            }
          }
          Spacer(minLength: 0)
          Button(action: createPane) {
            Label("Novo terminal", systemImage: "plus")
              .font(.system(size: 13, weight: .semibold)).frame(maxWidth: .infinity)
              .padding(.vertical, 13).modifier(Raised(radius: 12))
          }.disabled(!client.connected)
        }.frame(width: 218).padding(.vertical, 10)
        VStack(alignment: .leading, spacing: 0) {
          if let pane = client.selectedPane {
            HStack {
              Image(systemName: "terminal").foregroundStyle(Palette.accent)
              Text(pane.name).font(.system(size: 15, weight: .semibold, design: .rounded))
              if pane.generation > 1 {
                Text("shell novo").font(.system(size: 11)).foregroundStyle(Palette.secondary)
              }
              Spacer()
              Text(pane.phrase).font(.system(size: 12)).foregroundStyle(Palette.secondary)
              Button {
                paneToClose = pane.paneId
              } label: {
                Image(systemName: "xmark").padding(6)
              }
              .help("Fechar este terminal").disabled(!client.connected)
            }.padding(.horizontal, 20).padding(.vertical, 16)
            Text(pane.cwd).font(.system(size: 11, design: .monospaced)).foregroundStyle(
              Palette.secondary
            )
            .lineLimit(1).truncationMode(.middle).padding(.horizontal, 20).padding(.bottom, 12)
            Rectangle().fill(Palette.canvas).frame(height: 1)
            MacTerminal(client: client).padding(14)
            if let gap = client.gapText {
              Label(gap, systemImage: "exclamationmark.triangle")
                .font(.system(size: 12)).foregroundStyle(.orange).padding(12)
            }
          } else {
            Spacer()
            VStack(spacing: 18) {
              Image(systemName: "terminal").font(.system(size: 42, weight: .light)).foregroundStyle(
                Palette.secondary)
              Text("Seu espaço para começar").font(
                .system(size: 23, weight: .semibold, design: .rounded))
              Text(
                "Abra um terminal em uma pasta. O processo continua\nmesmo quando você fecha esta janela."
              )
              .font(.system(size: 14)).foregroundStyle(Palette.secondary).multilineTextAlignment(
                .center)
              Button("Escolher pasta", action: createPane)
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
      .sheet(isPresented: $showPairing, onDismiss: { client.clearPairCode() }) { pairing }
      .alert(
        "Fechar terminal?",
        isPresented: Binding(get: { paneToClose != nil }, set: { if !$0 { paneToClose = nil } })
      ) {
        Button("Cancelar", role: .cancel) { paneToClose = nil }
        Button("Fechar", role: .destructive) {
          if let id = paneToClose { client.closePane(id) }
          paneToClose = nil
        }
      } message: {
        Text("O shell desta pane será encerrado.")
      }
      .alert(
        "Não foi possível concluir",
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
            Text("Permitir acesso de \(request.deviceName)?").font(.headline)
            Text("Confira esta impressão digital no iPhone:").font(.subheadline)
            Text(request.fingerprint).font(.system(size: 12, design: .monospaced)).textSelection(
              .enabled)
            HStack {
              Button("Recusar") { client.decidePairing(request, approve: false) }
              Spacer()
              Button("Permitir") { client.decidePairing(request, approve: true) }.buttonStyle(
                .borderedProminent)
            }
          }.padding(22).frame(width: 420).modifier(Raised(radius: 20)).padding(.bottom, 40)
        }
      }
  }
  private func createPane() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.prompt = "Abrir terminal"
    panel.begin { response in
      if response == .OK, let directory = panel.url { client.createPane(directory: directory) }
    }
  }
  private var pairing: some View {
    ScrollView { pairingContent }.frame(width: 520, height: 720).background(Palette.canvas)
  }
  private var pairingContent: some View {
    VStack(spacing: 20) {
      Text("Continue no iPhone").font(.system(size: 24, weight: .semibold, design: .rounded))
      Text(
        "Abra o Soyeht Dev no iPhone e leia este código.\nOs dois dispositivos precisam estar na mesma tailnet."
      )
      .multilineTextAlignment(.center).foregroundStyle(Palette.secondary)
      if let code = client.pairCode, let image = qr(code.uri) {
        Image(nsImage: image).interpolation(.none).resizable().frame(width: 260, height: 260)
          .padding(16).background(.white, in: RoundedRectangle(cornerRadius: 16))
        Text("Código válido por 10 minutos.").font(.caption).foregroundStyle(Palette.secondary)
      } else {
        ProgressView().frame(width: 292, height: 292)
      }
      if let request = client.pairRequests.first {
        Text(request.deviceName).font(.headline)
        Text(request.fingerprint).font(.system(size: 11, design: .monospaced)).textSelection(
          .enabled)
        HStack {
          Button("Recusar") { client.decidePairing(request, approve: false) }
          Button("Permitir") {
            client.decidePairing(request, approve: true)
            showPairing = false
          }
          .buttonStyle(.borderedProminent)
        }
      }
      if !client.devices.isEmpty {
        VStack(alignment: .leading, spacing: 12) {
          Text("iPhones pareados").font(.headline)
          ForEach(client.devices, id: \.deviceId) { device in
            HStack {
              Label(device.name, systemImage: "iphone")
              Spacer()
              Button("Revogar", role: .destructive) { deviceToRevoke = device.deviceId }
            }
          }
        }.padding(.vertical, 12)
      }
      Button("Fechar") { showPairing = false }
    }.padding(32).frame(width: 460).background(Palette.canvas).foregroundStyle(Palette.ink)
      .confirmationDialog(
        "Revogar o acesso deste iPhone?",
        isPresented: Binding(
          get: { deviceToRevoke != nil }, set: { if !$0 { deviceToRevoke = nil } })
      ) {
        Button("Revogar", role: .destructive) {
          if let id = deviceToRevoke { client.revokeDevice(id) }
          deviceToRevoke = nil
        }
        Button("Cancelar", role: .cancel) { deviceToRevoke = nil }
      } message: {
        Text("As conexões serão fechadas. Um novo acesso exige outro pareamento.")
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
