import Foundation
import SwiftTerm

@MainActor
public final class TerminalHost: NSObject, @preconcurrency TerminalViewDelegate {
  static let background = (red: 232 / 255.0, green: 237 / 255.0, blue: 244 / 255.0)
  static let foreground = (red: 62 / 255.0, green: 74 / 255.0, blue: 102 / 255.0)

  let client: EngineClient
  private let acceptsInput: Bool
  private let ringBell: () -> Void
  private var openFile: (String, FilePathReference) -> Void
  private var openWeb: (String, URL) -> Void
  private var feeding = false
  private var selection: String?
  private var applyingRemoteSize = false

  init(
    client: EngineClient, acceptsInput: Bool, ringBell: @escaping () -> Void = {},
    openFile: @escaping (String, FilePathReference) -> Void = { _, _ in },
    openWeb: @escaping (String, URL) -> Void = { _, _ in }
  ) {
    self.client = client
    self.acceptsInput = acceptsInput
    self.ringBell = ringBell
    self.openFile = openFile
    self.openWeb = openWeb
  }

  func updateOpenFile(_ action: @escaping (String, FilePathReference) -> Void) {
    openFile = action
  }

  func updateOpenWeb(_ action: @escaping (String, URL) -> Void) { openWeb = action }

  func attach(_ view: TerminalView) {
    view.terminalDelegate = self
    client.terminalOutput = { [weak self, weak view] bytes, reset in
      guard let self, let view else { return }
      feeding = true
      defer { feeding = false }
      if reset { view.getTerminal().resetToInitialState() }
      view.feed(byteArray: Array(bytes)[...])
    }
    client.refreshTerminal()
  }

  func update(_ view: TerminalView) {
    guard client.connected, let pane = client.selectedPane else { return }
    let terminal = view.getTerminal()
    let revision = "\(pane.paneId):\(client.connectionRevision)"
    if selection != revision {
      selection = revision
      client.resize(cols: terminal.cols, rows: terminal.rows)
    } else if pane.cols != terminal.cols || pane.rows != terminal.rows {
      applyingRemoteSize = true
      terminal.resize(cols: Int(pane.cols), rows: Int(pane.rows))
      applyingRemoteSize = false
    }
  }

  func detach(_ view: TerminalView) {
    view.terminalDelegate = nil
    client.terminalOutput = nil
  }

  public func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
    if !applyingRemoteSize { client.resize(cols: newCols, rows: newRows) }
  }

  public func send(source: TerminalView, data: ArraySlice<UInt8>) {
    // Replayed control sequences must not become new input to the shell.
    if acceptsInput && !feeding { client.input(Data(data)) }
  }

  public func setTerminalTitle(source: TerminalView, title: String) {}
  public func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
  public func scrolled(source: TerminalView, position: Double) {}
  public func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
    guard let workspaceId = client.selectedPane?.workspaceId else { return }
    if let reference = FilePathReference.parse(link) {
      openFile(workspaceId, reference)
    } else if let destination = WebLinkRouting.destination(
      for: link, openExternalLinksInSoyeht: true)
    {
      switch destination {
      case .soyeht(let url), .system(let url): openWeb(workspaceId, url)
      }
    }
  }
  public func bell(source: TerminalView) { ringBell() }
  public func clipboardCopy(source: TerminalView, content: Data) {}
  public func clipboardRead(source: TerminalView) -> Data? { nil }
  public func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
  public func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
}
