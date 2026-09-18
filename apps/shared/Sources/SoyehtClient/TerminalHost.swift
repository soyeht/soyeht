import Foundation
import SwiftTerm

@MainActor
public final class TerminalHost: NSObject, @preconcurrency TerminalViewDelegate {
  let client: EngineClient
  private var acceptsInput: Bool
  private let sendInput: (Data) -> Void
  private let ringBell: () -> Void
  private var openFile: (String, FilePathReference) -> Void
  private var openWeb: (String, URL) -> Void
  private var feeding = false
  private var selection: String?
  private var applyingRemoteSize = false

  init(
    client: EngineClient, acceptsInput: Bool, ringBell: @escaping () -> Void = {},
    openFile: @escaping (String, FilePathReference) -> Void = { _, _ in },
    openWeb: @escaping (String, URL) -> Void = { _, _ in },
    sendInput: ((Data) -> Void)? = nil
  ) {
    self.client = client
    self.acceptsInput = acceptsInput
    self.sendInput = sendInput ?? { client.input($0) }
    self.ringBell = ringBell
    self.openFile = openFile
    self.openWeb = openWeb
  }

  func updateOpenFile(_ action: @escaping (String, FilePathReference) -> Void) {
    openFile = action
  }

  func updateOpenWeb(_ action: @escaping (String, URL) -> Void) { openWeb = action }

  func updateAcceptsInput(_ acceptsInput: Bool) { self.acceptsInput = acceptsInput }

  func attach(_ view: TerminalView, refresh: Bool = true) {
    view.terminalDelegate = self
    client.claimTerminalOutput(owner: self) { [weak self, weak view] bytes, reset in
      guard let self, let view else { return }
      feeding = true
      defer { feeding = false }
      if reset { view.getTerminal().resetToInitialState() }
      view.feed(byteArray: Array(bytes)[...])
    }
    if refresh { client.refreshTerminal() }
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
    if view.terminalDelegate === self { view.terminalDelegate = nil }
    client.releaseTerminalOutput(owner: self)
  }

  public func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
    if !applyingRemoteSize { client.resize(cols: newCols, rows: newRows) }
  }

  public func send(source: TerminalView, data: ArraySlice<UInt8>) {
    // Replayed control sequences must not become new input to the shell.
    if acceptsInput && !feeding { sendInput(Data(data)) }
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
