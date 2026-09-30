import Foundation
import SoyehtProtocol
import SwiftTerm

@MainActor
public final class TerminalHost: NSObject, @preconcurrency TerminalViewDelegate {
  let client: EngineClient
  private let paneId: String?
  private var acceptsInput: Bool
  private let sendInput: (Data) -> Void
  private let resizeTerminal: (String?, Int, Int) -> Void
  private let ringBell: () -> Void
  private var openFile: (String, FilePathReference) -> Void
  private var openWeb: (String, URL) -> Void
  private var feeding = false
  private var selection: String?
  private var applyingRemoteSize = false

  init(
    client: EngineClient, paneId: String? = nil, acceptsInput: Bool,
    ringBell: @escaping () -> Void = {},
    openFile: @escaping (String, FilePathReference) -> Void = { _, _ in },
    openWeb: @escaping (String, URL) -> Void = { _, _ in },
    sendInput: ((Data) -> Void)? = nil,
    resizeTerminal: ((String?, Int, Int) -> Void)? = nil
  ) {
    self.client = client
    self.paneId = paneId
    self.acceptsInput = acceptsInput
    self.sendInput =
      sendInput ?? { data in
        if let paneId { client.input(data, paneId: paneId) } else { client.input(data) }
      }
    self.resizeTerminal =
      resizeTerminal ?? { paneId, cols, rows in
        if let paneId {
          client.resize(paneId: paneId, cols: cols, rows: rows)
        } else {
          client.resize(cols: cols, rows: rows)
        }
      }
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
    if paneId == nil {
      client.claimTerminalOutput(owner: self) { [weak self, weak view] bytes, reset in
        guard let self, let view else { return }
        feeding = true
        defer { feeding = false }
        if reset { view.getTerminal().resetToInitialState() }
        view.feed(byteArray: Array(bytes)[...])
      }
    }
    if refresh {
      if let paneId { client.refreshTerminal(paneId) } else { client.refreshTerminal() }
    }
  }

  func update(_ view: TerminalView) {
    guard client.connected, let pane = representedPane else { return }
    let terminal = view.getTerminal()
    let revision = "\(pane.paneId):\(client.connectionRevision)"
    if selection != revision {
      selection = revision
      resizeTerminal(paneId, terminal.cols, terminal.rows)
    } else if pane.cols != terminal.cols || pane.rows != terminal.rows {
      applyingRemoteSize = true
      terminal.resize(cols: Int(pane.cols), rows: Int(pane.rows))
      applyingRemoteSize = false
    }
  }

  func detach(_ view: TerminalView) {
    if view.terminalDelegate === self { view.terminalDelegate = nil }
    if paneId == nil { client.releaseTerminalOutput(owner: self) }
  }

  public func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
    if !applyingRemoteSize { resizeTerminal(paneId, newCols, newRows) }
  }

  public func send(source: TerminalView, data: ArraySlice<UInt8>) {
    // Replayed control sequences must not become new input to the shell.
    #if os(macOS)
      let isReceivingOutput = (source as? TerminalSurface)?.isReceivingOutput == true
    #else
      let isReceivingOutput = false
    #endif
    if acceptsInput && !feeding && !isReceivingOutput { sendInput(Data(data)) }
  }

  public func setTerminalTitle(source: TerminalView, title: String) {}
  public func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
  public func scrolled(source: TerminalView, position: Double) {}
  public func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
    guard let workspaceId = representedPane?.workspaceId else { return }
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

  private var representedPane: Pane? {
    if let paneId { return client.panes.first { $0.paneId == paneId } }
    return client.selectedPane
  }
}
