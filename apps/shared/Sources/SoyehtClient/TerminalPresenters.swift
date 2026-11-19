import Foundation
import SoyehtProtocol

/// What the presenters need from the client to keep a terminal's stream alive: the engine's
/// subscribe and unsubscribe requests, and the replica and sink that follow them.
@MainActor protocol TerminalStreamControl: AnyObject {
  var isConnected: Bool { get }
  /// A live terminal pane of the engine.
  func isTerminal(_ paneId: String) -> Bool
  func hasStream(_ paneId: String) -> Bool
  /// A single-pane screen shows this pane, on the output sink that follows the selection.
  func isShownAlone(_ paneId: String) -> Bool
  /// Asks the engine for the pane's screen from its tail; the output goes to the pane's sink.
  func subscribeStream(_ paneId: String) async throws
  func unsubscribeStream(_ paneId: String) async throws
  /// Forgets the pane's stream, its in-flight request and its sink.
  func dropStream(_ paneId: String)
  func run(_ action: @escaping @MainActor () async throws -> Void)
}

/// The terminal views of a paired device that show a pane at the same time, such as the
/// terminals of a pane board. The device used to follow one terminal: focusing another pane
/// dropped the previous stream. With several views on screen, a stream stays while any view
/// presents its pane, whatever the focus, and goes when the last one goes. This is the one place
/// that rule lives.
///
/// Views are counted by owner, so two views of one pane (a card rebuilt before the old one is
/// taken down, a board that gave way to the single-pane screen) never end each other's stream.
@MainActor final class TerminalPresenters {
  private weak var control: TerminalStreamControl?
  private var owners: [String: Set<ObjectIdentifier>] = [:]

  init(control: TerminalStreamControl) {
    self.control = control
  }

  /// The panes a view presents now.
  var paneIds: Set<String> { Set(owners.keys) }

  func isPresented(_ paneId: String) -> Bool { owners[paneId]?.isEmpty == false }

  /// A view shows `paneId` and has claimed the pane's output sink. The pane's screen is asked
  /// for again even when a stream exists, because the new view starts empty: a replay resets and
  /// refills it, and goes to the newest sink. Without a connection nothing is asked; the
  /// connection's `resubscribe` does it.
  func attach(_ owner: AnyObject, paneId: String) {
    owners[paneId, default: []].insert(ObjectIdentifier(owner))
    subscribeIfPossible(paneId)
  }

  /// A view stopped showing `paneId`. The stream goes with the last view, unless a single-pane
  /// screen has taken the pane over: that screen subscribed on its own, and ending the stream
  /// now would end its output.
  func detach(_ owner: AnyObject, paneId: String) {
    guard owners[paneId]?.remove(ObjectIdentifier(owner)) != nil else { return }
    guard owners[paneId]?.isEmpty == true else { return }
    owners.removeValue(forKey: paneId)
    guard let control, !control.isShownAlone(paneId) else { return }
    control.dropStream(paneId)
    guard control.isConnected, control.isTerminal(paneId) else { return }
    control.run { try await control.unsubscribeStream(paneId) }
  }

  /// After a connection is made: every pane a view presents gets its stream again.
  func resubscribe() async throws {
    guard let control else { return }
    for paneId in paneIds.sorted() where !control.hasStream(paneId) && control.isTerminal(paneId) {
      try await control.subscribeStream(paneId)
    }
  }

  private func subscribeIfPossible(_ paneId: String) {
    guard let control, control.isConnected, control.isTerminal(paneId) else { return }
    control.run { try await control.subscribeStream(paneId) }
  }
}

extension EngineClient: TerminalStreamControl {
  /// A gap notice changes when a gap does, not when a chunk arrives.
  func syncGapTexts() {
    if terminalGapTexts != terminalStreams.gapTextByPane {
      terminalGapTexts = terminalStreams.gapTextByPane
    }
  }
  var isConnected: Bool { connected }
  func isTerminal(_ paneId: String) -> Bool {
    panes.contains { $0.paneId == paneId && $0.mode == .terminal }
  }
  func hasStream(_ paneId: String) -> Bool { terminalStreams.contains(paneId) }
  func isShownAlone(_ paneId: String) -> Bool { terminalOutput != nil && selectedPaneId == paneId }
  func subscribeStream(_ paneId: String) async throws {
    try await subscribeWithoutFailingTheSession(paneId)
  }

  /// A terminal the engine refuses, because it closed since the pane list was read, does not end
  /// the session: the failure is logged by its kind and the next terminal is subscribed.
  func subscribeWithoutFailingTheSession(_ paneId: String) async throws {
    do {
      try await subscribe(paneId)
    } catch ClientFailure.remote(let kind, _, _) {
      clientLogger.error("Terminal subscribe refused: \(kind, privacy: .public)")
    }
  }
  func unsubscribeStream(_ paneId: String) async throws {
    _ = try await request(.paneStreamUnsubscribe, .object(["paneId": .string(paneId)]))
  }
  func dropStream(_ paneId: String) { discardTerminalPane(paneId) }
  func run(_ action: @escaping @MainActor () async throws -> Void) { perform(action) }

  /// A terminal view of a board shows `paneId`; it has claimed the pane's output sink.
  func attachBoardTerminal(paneId: String, owner: AnyObject) {
    terminalPresenters.attach(owner, paneId: paneId)
  }

  /// The terminal view of a board went away.
  func detachBoardTerminal(paneId: String, owner: AnyObject) {
    // A newer view may hold the sink; only the view that holds it releases it.
    releaseTerminalOutput(paneId: paneId, owner: owner)
    terminalPresenters.detach(owner, paneId: paneId)
  }

  /// Whether this platform keeps every terminal subscribed, not only those on screen.
  static var followsEveryTerminal: Bool {
    #if os(macOS)
      true
    #else
      false
    #endif
  }

  /// The step of a session after its snapshot, handshake and subscriptions: the connection is
  /// declared up and every surface that shows a pane gets its content. Both platforms take it,
  /// the Mac for the terminals it has a surface for and a paired device for those on screen.
  ///
  /// The selected pane comes first, then the other terminals of the workspace on screen, then the
  /// surfaces of other workspaces. Every subscribe waits for the previous replay to be read
  /// (`TerminalReplayGate`), so the engine never queues more than one tail for the connection, and
  /// no replay of up to 2 MiB parsed on the main actor waits behind fifty of them. A terminal with
  /// no surface yet is not subscribed: its output would go nowhere. It asks for its screen when
  /// its surface appears.
  func settleConnection(followsEveryTerminal: Bool = EngineClient.followsEveryTerminal) async throws
  {
    connectionRevision &+= 1
    connected = true
    connectionText = ActiveProfile.values.connectedText
    if selectedPaneId == nil { selectedPaneId = panes.first?.paneId }
    guard followsEveryTerminal else {
      // A paired device follows its selected terminal and the terminals its views show.
      try await subscribeShownTerminals()
      try await loadSelectedPane()
      return
    }
    // The tmux-style Mac surface keeps every terminal it has current, including hidden ones.
    let (shown, others) = macTerminalsToFollow()
    // `macTerminalsToFollow` puts the selected terminal first when it has a surface.
    let selected = shown.first { $0 == selectedPaneId }
    if let selected { try await subscribeWithoutFailingTheSession(selected) }
    try await loadSelectedPane()
    // A pane that was subscribed meanwhile (the person clicked it) is not replayed again.
    for paneId in shown.filter({ $0 != selected }) + others where connected && !hasStream(paneId) {
      try await subscribeWithoutFailingTheSession(paneId)
    }
  }

  private func loadSelectedPane() async throws {
    guard let pane = selectedPane else { return }
    switch pane.mode {
    case .agent:
      await hydratePresentedAgentPanes([PresentedPane(paneId: pane.paneId, isVisible: true)])
    case .editor: await loadEditorPane(pane.paneId)
    case .writing: try await loadWritingPane(pane.paneId)
    case .terminal, .web: break
    }
  }

  /// The terminals the Mac has a surface for: those of the selected workspace, the selected pane
  /// first, and the others.
  private func macTerminalsToFollow() -> (shown: [String], others: [String]) {
    let workspaceId = selectedPane?.workspaceId
    let surfaces = panes.filter { $0.mode == .terminal && hasTerminalSurface($0.paneId) }
    var shown = surfaces.filter { $0.workspaceId == workspaceId }.map(\.paneId)
    let others = surfaces.filter { $0.workspaceId != workspaceId }.map(\.paneId)
    if let selected = selectedPaneId, let index = shown.firstIndex(of: selected) {
      shown.insert(shown.remove(at: index), at: 0)
    }
    return (shown, others)
  }

  /// After the snapshot of a connection: the selected terminal and every terminal a view
  /// presents get their streams. The device subscribes by what is on screen, since the views of
  /// panes that the reconnect did not remove stay where they are and never mount again.
  func subscribeShownTerminals() async throws {
    if let selected = selectedPaneId, isTerminal(selected), !hasStream(selected) {
      try await subscribeWithoutFailingTheSession(selected)
    }
    try await terminalPresenters.resubscribe()
  }

  /// The selection left `old`. A pane's stream is kept while a view still presents it.
  func unsubscribeAfterSelecting(from old: String) async throws {
    guard isTerminal(old), !terminalPresenters.isPresented(old) else { return }
    try await unsubscribeStream(old)
    forgetTerminalStream(old)
  }

  /// The agent panes among `paneIds`, as the surfaces `hydratePresentedAgentPanes` loads: every
  /// pane of a board is visible at once, unlike the panes the phone navigates between.
  public func presentedAgents(in paneIds: [String]) -> [PresentedPane] {
    let agents = Set(panes.filter { $0.mode == .agent }.map(\.paneId))
    return paneIds.filter(agents.contains).map { PresentedPane(paneId: $0, isVisible: true) }
  }
}
