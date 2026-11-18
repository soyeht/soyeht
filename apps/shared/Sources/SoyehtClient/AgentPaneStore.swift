import Foundation
import Observation

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
  /// How many times the transcript was grouped.
  @ObservationIgnored private(set) var groupings = 0

  init(_ view: AgentPaneView = AgentPaneView()) {
    self.view = view
    attention = AgentPaneAttention(view)
  }

  /// The transcript with each subagent's activity under its row.
  public var entries: [AgentTranscriptEntry] {
    access(keyPath: \.view)
    if let grouped { return grouped }
    groupings += 1
    let entries = AgentTranscriptGrouping.entries(view.messages)
    grouped = entries
    return entries
  }

  private func viewChanged() {
    grouped = nil
    let next = AgentPaneAttention(view)
    if next != attention { attention = next }
  }
}

/// The agent panes of a client, one model each. Reading a pane follows that pane alone; only a
/// pane that appears or goes away is seen by every reader.
@MainActor @Observable
public final class AgentPaneStore {
  private var models: [String: AgentPaneModel] = [:]

  public init() {}

  public func model(_ paneId: String) -> AgentPaneModel? { models[paneId] }

  func attention(for paneId: String) -> AgentPaneAttention {
    models[paneId]?.attention ?? AgentPaneAttention(nil)
  }

  /// Questions still waiting for a person, across every pane.
  var openRequestIds: Set<String> {
    Set(
      models.values.flatMap { [$0.attention.permission?.requestId, $0.attention.elicitation?.requestId] }
        .compactMap { $0 })
  }

  private func model(creating paneId: String, from view: AgentPaneView = AgentPaneView())
    -> AgentPaneModel
  {
    if let model = models[paneId] { return model }
    let model = AgentPaneModel(view)
    models[paneId] = model
    return model
  }

  public subscript(_ paneId: String) -> AgentPaneView? {
    get { models[paneId]?.view }
    set {
      if let newValue {
        model(creating: paneId, from: newValue).view = newValue
      } else {
        models.removeValue(forKey: paneId)
      }
    }
  }

  /// The pane's view, made when the pane has none. A change is made in place.
  public subscript(
    _ paneId: String, default makeDefault: @autoclosure () -> AgentPaneView
  ) -> AgentPaneView {
    get { models[paneId]?.view ?? makeDefault() }
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
    for paneId in models.keys where !paneIds.contains(paneId) { models.removeValue(forKey: paneId) }
  }

  func removeAll() {
    guard !models.isEmpty else { return }
    models.removeAll()
  }
}
