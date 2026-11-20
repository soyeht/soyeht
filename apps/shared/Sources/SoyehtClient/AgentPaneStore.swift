import Foundation
import Observation
import SoyehtProtocol

/// What a pane asks of the person, which is all the tab bar and the rail need of it. It changes
/// when a request opens or closes, not when the pane streams, so the views that follow it are
/// not redrawn by a chunk.
struct AgentPaneAttention: Equatable, Sendable {
  struct Key: Equatable, Sendable {
    let requestId: String
    let generation: UInt32
  }

  var permission: Key?
  var elicitation: Key?

  init(_ view: AgentPaneView?) {
    permission = view?.permission.map { Key(requestId: $0.requestId, generation: $0.generation) }
    elicitation = view?.elicitation.map { Key(requestId: $0.requestId, generation: $0.generation) }
  }
}

/// One agent pane's state, observed on its own: a chunk for this pane redraws the views that read
/// this model and no others. The grouped transcript is made when it is read after the messages
/// changed, so a pane nobody is looking at never pays for it, and a view body does not repeat it.
@MainActor @Observable
public final class AgentPaneModel {
  public var view: AgentPaneView { didSet { viewChanged() } }
  private(set) var attention: AgentPaneAttention
  @ObservationIgnored private var grouped: [AgentTranscriptEntry]?
  @ObservationIgnored private let group: ([TranscriptMessage]) -> [AgentTranscriptEntry]
  /// Told when the pane's requests change, which is when the attention summary can change.
  @ObservationIgnored var attentionChanged: (@MainActor () -> Void)?

  init(
    _ view: AgentPaneView = AgentPaneView(),
    group: @escaping ([TranscriptMessage]) -> [AgentTranscriptEntry] =
      AgentTranscriptGrouping.entries
  ) {
    self.view = view
    self.group = group
    attention = AgentPaneAttention(view)
  }

  /// The transcript with each subagent's activity under its row.
  public var entries: [AgentTranscriptEntry] {
    access(keyPath: \.view)
    if let grouped { return grouped }
    let entries = group(view.messages)
    grouped = entries
    return entries
  }

  private func viewChanged() {
    grouped = nil
    let next = AgentPaneAttention(view)
    if next != attention {
      attention = next
      attentionChanged?()
    }
  }
}

/// The agent panes of a client, one model each. Reading a pane follows that pane alone; only a
/// pane that appears or goes away is seen by every reader.
@MainActor @Observable
public final class AgentPaneStore {
  private var models: [String: AgentPaneModel] = [:]
  /// The views that show each pane, by owner, so two views of one pane (a card rebuilt before the
  /// old one is taken down) never end each other's showing. It outlives a pane's model: a
  /// reconnect drops the models while the views stay mounted and never report again.
  @ObservationIgnored private var viewers: [String: Set<ObjectIdentifier>] = [:]

  /// The panes a turn ended in while no view showed them: what the turn brought (the person's
  /// prompt, the messages of other agents) comes with the reload that was skipped. The mark stays
  /// until a page that was asked for after it arrives, so a reload that fails is made again.
  @ObservationIgnored private var stale: Set<String> = []
  @ObservationIgnored private var reading: Set<String> = []
  /// Told when a pane's requests change or a pane appears or goes: the attention summary reads them.
  @ObservationIgnored var attentionChanged: (@MainActor () -> Void)?

  public init() {}

  func markStale(_ paneId: String) {
    stale.insert(paneId)
    reading.remove(paneId)
  }

  func isStale(_ paneId: String) -> Bool { stale.contains(paneId) }

  /// A pane that is closed leaves no mark behind.
  func forget(_ paneId: String) {
    stale.remove(paneId)
    reading.remove(paneId)
  }

  /// A page is asked for: if it arrives, it covers what made the pane stale until now.
  func beginRead(_ paneId: String) {
    if stale.contains(paneId) { reading.insert(paneId) }
  }

  func finishRead(_ paneId: String) {
    if reading.remove(paneId) != nil { stale.remove(paneId) }
  }

  /// Whether a view shows the pane, which sets how much of its conversation the replica holds.
  func isShown(_ paneId: String) -> Bool { viewers[paneId]?.isEmpty == false }

  /// A view starts or stops showing the pane; true when the pane went from shown to not or back.
  func setViewer(_ owner: AnyObject, _ paneId: String, shown: Bool) -> Bool {
    let was = isShown(paneId)
    if shown {
      viewers[paneId, default: []].insert(ObjectIdentifier(owner))
    } else {
      viewers[paneId]?.remove(ObjectIdentifier(owner))
      if viewers[paneId]?.isEmpty == true { viewers.removeValue(forKey: paneId) }
    }
    let now = isShown(paneId)
    if let scroll = models[paneId]?.view.scroll { scroll.isShown = now }
    return now != was
  }

  public func model(_ paneId: String) -> AgentPaneModel? { models[paneId] }

  func attention(for paneId: String) -> AgentPaneAttention {
    models[paneId]?.attention ?? AgentPaneAttention(nil)
  }

  /// Whether the pane has a permission or a question open. Reading it follows the pane's requests,
  /// not its transcript.
  public func isAsking(_ paneId: String) -> Bool {
    let asks = attention(for: paneId)
    return asks.permission != nil || asks.elicitation != nil
  }

  /// Questions still waiting for a person, across every pane.
  var openRequestIds: Set<String> {
    Set(
      models.values.flatMap {
        [$0.attention.permission?.requestId, $0.attention.elicitation?.requestId]
      }
      .compactMap { $0 })
  }

  private func model(creating paneId: String, from view: AgentPaneView = AgentPaneView())
    -> AgentPaneModel
  {
    if let model = models[paneId] { return model }
    view.scroll.isShown = isShown(paneId)
    let model = AgentPaneModel(view)
    model.attentionChanged = { [weak self] in self?.attentionChanged?() }
    models[paneId] = model
    attentionChanged?()
    return model
  }

  public subscript(_ paneId: String) -> AgentPaneView? {
    get { models[paneId]?.view }
    set {
      if let newValue {
        newValue.scroll.isShown = isShown(paneId)
        model(creating: paneId, from: newValue).view = newValue
      } else {
        guard models[paneId] != nil else { return }
        models.removeValue(forKey: paneId)
        attentionChanged?()
      }
    }
  }

  /// The pane's view, made when the pane has none. A change is made in place.
  public subscript(
    _ paneId: String, default makeDefault: @autoclosure () -> AgentPaneView
  ) -> AgentPaneView {
    get { models[paneId]?.view ?? makeDefault() }
    // A plain get and set would copy the message array of the pane on every chunk.
    _modify {
      let model = model(creating: paneId, from: makeDefault())
      yield &model.view
    }
  }

  /// Changes the pane's view in one step: its readers are told once.
  func update(_ paneId: String, _ change: (inout AgentPaneView) -> Void) {
    change(&self[paneId, default: AgentPaneView()])
  }

  /// Drops the panes not in `paneIds`; readers hear of it only when one went.
  func retainOnly(_ paneIds: Set<String>) {
    let gone = models.keys.filter { !paneIds.contains($0) }
    for paneId in gone { models.removeValue(forKey: paneId) }
    for paneId in gone { forget(paneId) }
    if !gone.isEmpty { attentionChanged?() }
  }

  func removeAll() {
    guard !models.isEmpty else { return }
    models.removeAll()
    attentionChanged?()
  }
}

/// One view of a conversation, which the store counts as showing the pane while it is on screen.
final class AgentPaneViewer {}
