import Foundation

public enum PhoneWritingTab: String, Codable, Equatable, Sendable {
  case document
  case agent
}

public struct PhoneWritingIdentity: Codable, Equatable, Hashable, Sendable {
  public var paneId: String
  public var fileId: String?

  public init(paneId: String, fileId: String?) {
    self.paneId = paneId
    self.fileId = fileId
  }
}

public struct PhoneWritingDocumentDescriptor: Equatable, Sendable {
  public var paneId: String
  public var fileId: String
  /// The complete title. Views may truncate its presentation but never this value.
  public var name: String
  public var kind: PaneLayoutKind
  public var font: PaneLayoutFontMetric
  public var fullWidth: Bool

  public init(
    paneId: String, fileId: String, name: String, kind: PaneLayoutKind,
    font: PaneLayoutFontMetric, fullWidth: Bool = false
  ) {
    self.paneId = paneId
    self.fileId = fileId
    self.name = name
    self.kind = kind
    self.font = font
    self.fullWidth = fullWidth
  }
}

public enum PhoneWritingAvailability: Equatable, Sendable {
  case ready
  case missingDocument
  case inconsistentDocument
}

public struct PhoneWritingPaneState: Codable, Equatable, Sendable {
  public var identity: PhoneWritingIdentity
  public var expanded: Bool
  public var activeTab: PhoneWritingTab
  /// A multiplier over the screenplay's whole-page fit. One is always the reset value.
  public var screenplayZoom: Double

  public init(
    identity: PhoneWritingIdentity, expanded: Bool = false,
    activeTab: PhoneWritingTab = .document, screenplayZoom: Double = 1
  ) {
    self.identity = identity
    self.expanded = expanded
    self.activeTab = activeTab
    self.screenplayZoom = screenplayZoom
  }
}

/// Versioned device-local viewport state. It is never written to `workspace.layout`.
public struct PhoneWritingLocalState: Codable, Equatable, Sendable {
  public static let currentVersion: UInt32 = 1

  public var version: UInt32
  public var panes: [PhoneWritingPaneState]

  public init(version: UInt32 = Self.currentVersion, panes: [PhoneWritingPaneState] = []) {
    self.version = version
    self.panes = panes
  }

  public func encoded() throws -> Data {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    return try encoder.encode(self)
  }

  public static func decoded(_ data: Data) -> Self? {
    guard let state = ClientDiagnostics.decoded(Self.self, from: data, event: "phone_state_broken"),
      state.version == currentVersion
    else { return nil }
    return state
  }

  /// Opens a newly created or explicitly chosen document without changing the state of
  /// any other leaf. Replacing by pane id also prevents a transient missing-document
  /// identity from leaving a duplicate behind once its file id arrives.
  public func opening(_ identity: PhoneWritingIdentity) -> Self {
    var updated = self
    if let index = updated.panes.firstIndex(where: { $0.identity == identity }) {
      updated.panes[index].expanded = true
      return updated
    }
    updated.panes.removeAll { $0.identity.paneId == identity.paneId }
    updated.panes.append(PhoneWritingPaneState(identity: identity, expanded: true))
    return updated
  }
}

public enum PhoneWritingStateAction: Equatable, Sendable {
  case toggle(PhoneWritingIdentity)
  case selectTab(PhoneWritingIdentity, PhoneWritingTab)
  case setScreenplayZoom(PhoneWritingIdentity, Double)
}

/// Phone navigation is device-local. Only an explicit operation on a remote pane may
/// move the engine's shared focus.
public enum PhoneWritingFocusAction: Equatable, Sendable {
  case enterWorkspace
  case toggleLeaf
  case selectLocalTab
  case restoreLocalState
  case sendToAgent(String)
}

public enum PhoneWritingFocusPolicy {
  public static func apply(_ action: PhoneWritingFocusAction, select: (String) -> Void) {
    guard case .sendToAgent(let paneId) = action, !paneId.isEmpty else { return }
    select(paneId)
  }
}
