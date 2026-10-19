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
  private let writeClipboard: (String, TerminalView) -> Void
  private let replayOverride: (() -> Bool)?
  private var openFile: (String, FilePathReference) -> Void
  private var openWeb: (String, URL) -> Void
  private var feeding = false
  private var selection: String?
  private var applyingRemoteSize = false
  private var sizeMode: TerminalSizeMode
  private var requestedSize: (grid: TerminalGrid, at: ContinuousClock.Instant)?
  /// How long a size asked of the engine is waited for before the pane's size is trusted again.
  private static let requestGrace = Duration.seconds(2)

  init(
    client: EngineClient, paneId: String? = nil, acceptsInput: Bool,
    ringBell: @escaping () -> Void = {},
    openFile: @escaping (String, FilePathReference) -> Void = { _, _ in },
    openWeb: @escaping (String, URL) -> Void = { _, _ in },
    sizeMode: TerminalSizeMode = .fitThisScreen,
    writeClipboard: @escaping (String, TerminalView) -> Void = { _, _ in },
    isReplaying: (() -> Bool)? = nil,
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
    self.sizeMode = sizeMode
    self.writeClipboard = writeClipboard
    self.replayOverride = isReplaying
    self.ringBell = ringBell
    self.openFile = openFile
    self.openWeb = openWeb
  }

  func updateOpenFile(_ action: @escaping (String, FilePathReference) -> Void) {
    openFile = action
  }

  func updateOpenWeb(_ action: @escaping (String, URL) -> Void) { openWeb = action }

  func updateAcceptsInput(_ acceptsInput: Bool) { self.acceptsInput = acceptsInput }

  /// Returns whether the mode changed, so the caller can have the view measure itself again.
  @discardableResult
  func updateSizeMode(_ mode: TerminalSizeMode) -> Bool {
    guard mode != sizeMode else { return false }
    sizeMode = mode
    selection = nil
    requestedSize = nil
    return true
  }

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

  /// Replays the screen after the view changed its grid, so the lines are laid out again from
  /// the pane's own bytes instead of reflowed in place.
  func redraw() {
    if let paneId { client.refreshTerminal(paneId) } else { client.refreshTerminal() }
  }

  func update(_ view: TerminalView) {
    guard client.connected, let pane = representedPane else { return }
    update(view, pane: pane, connectionRevision: client.connectionRevision)
  }

  func update(_ view: TerminalView, pane: Pane, connectionRevision: UInt64) {
    let terminal = view.getTerminal()
    let revision = "\(pane.paneId):\(connectionRevision)"
    let action = TerminalSizePolicy.reconcile(
      mode: sizeMode, pane: grid(of: pane),
      local: TerminalGrid(cols: terminal.cols, rows: terminal.rows),
      announced: selection == revision, requestInFlight: requestInFlight(for: pane))
    selection = revision
    perform(action, on: terminal)
  }

  private func grid(of pane: Pane) -> TerminalGrid {
    TerminalGrid(cols: Int(pane.cols), rows: Int(pane.rows))
  }

  private func perform(_ action: TerminalSizeAction, on terminal: Terminal) {
    switch action {
    case .none:
      break
    case .request(let grid):
      requestedSize = (grid, .now)
      resizeTerminal(paneId, grid.cols, grid.rows)
    case .adopt(let grid):
      applyingRemoteSize = true
      terminal.resize(cols: grid.cols, rows: grid.rows)
      applyingRemoteSize = false
    }
  }

  private func requestInFlight(for pane: Pane) -> Bool {
    guard let requestedSize else { return false }
    return grid(of: pane) != requestedSize.grid
      && requestedSize.at.duration(to: .now) < Self.requestGrace
  }

  func detach(_ view: TerminalView) {
    if view.terminalDelegate === self { view.terminalDelegate = nil }
    if paneId == nil { client.releaseTerminalOutput(owner: self) }
  }

  public func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
    guard !applyingRemoteSize else { return }
    perform(
      TerminalSizePolicy.layoutChanged(
        mode: sizeMode, pane: representedPane.map(grid(of:)),
        natural: TerminalGrid(cols: newCols, rows: newRows)),
      on: source.getTerminal())
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
  public func clipboardCopy(source: TerminalView, content: Data) {
    // A replay plays recorded output again, and its copy requests were answered when they
    // first happened: only what the program writes now may reach the clipboard.
    guard !isReplaying, let text = TerminalClipboardPolicy.copiedText(content) else { return }
    writeClipboard(text, source)
  }
  /// A program in the terminal never reads the clipboard; the person pastes on purpose.
  public func clipboardRead(source: TerminalView) -> Data? { nil }
  public func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
  public func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}

  private var isReplaying: Bool {
    if let replayOverride { return replayOverride() }
    return representedPane.map { client.terminalStreams.isReplaying($0.paneId) } ?? false
  }

  private var representedPane: Pane? {
    if let paneId { return client.panes.first { $0.paneId == paneId } }
    return client.selectedPane
  }
}
