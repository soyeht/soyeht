import Foundation
import SoyehtProtocol

public struct WebPaneView: Sendable {
  public var navigationGeneration: String?
  public var warnings: UInt32
  public var errors: UInt32
  public var agentDriving: Bool
  public var drivingDomain: String?

  public init(
    navigationGeneration: String? = nil, warnings: UInt32 = 0, errors: UInt32 = 0,
    agentDriving: Bool = false, drivingDomain: String? = nil
  ) {
    self.navigationGeneration = navigationGeneration
    self.warnings = warnings
    self.errors = errors
    self.agentDriving = agentDriving
    self.drivingDomain = drivingDomain
  }
}

public struct WebControlHandoff: Sendable {
  public private(set) var humanHasControl = false
  public private(set) var agentCommandRunning = false

  public init() {}

  public mutating func noteHumanInput() {
    humanHasControl = true
  }

  public mutating func beginAgentCommand() -> Bool {
    guard !humanHasControl else { return false }
    agentCommandRunning = true
    return true
  }

  public mutating func finishAgentCommand() {
    agentCommandRunning = false
  }

  public mutating func handBack() {
    humanHasControl = false
  }
}

@MainActor
public protocol WebHostPort: AnyObject {
  func execute(_ command: WebCommandParams) async throws -> WebHostOutput
}

public struct WebHostAttachment: Sendable {
  public let reference: AttachmentRef
  public let bytes: Data

  public init(reference: AttachmentRef, bytes: Data) {
    self.reference = reference
    self.bytes = bytes
  }
}

public struct WebHostOutput: Sendable {
  public var result: WebCommandResult
  public var attachments: [WebHostAttachment]

  public init(result: WebCommandResult, attachments: [WebHostAttachment] = []) {
    self.result = result
    self.attachments = attachments
  }
}

extension EngineClient {
  public var selectedWebPane: Pane? {
    guard selectedPane?.mode == .web else { return nil }
    return selectedPane
  }

  func applyWebSnapshot(panes: [Pane]) {
    webPanes = Dictionary(
      uniqueKeysWithValues: panes.lazy.filter { $0.mode == .web }.map {
        ($0.paneId, WebPaneView())
      })
  }

  func applyWebEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .serversLocalChanged:
      localServers = try params.decoded(ServersLocalChangedEvent.self).servers
    case .webNavigated:
      let event = try params.decoded(WebNavigatedEvent.self)
      updateWebPane(event.paneId) { $0.navigationGeneration = event.navigationGeneration }
      if var pane = panes.first(where: { $0.paneId == event.paneId }) {
        pane.url = event.url
        pane.title = event.title
        upsertPane(pane)
      }
    case .webConsoleCounts:
      let event = try params.decoded(WebConsoleCountsEvent.self)
      updateWebPane(event.paneId) {
        $0.warnings = event.warnings
        $0.errors = event.errors
      }
    case .webAgentDriving:
      let event = try params.decoded(WebAgentDrivingEvent.self)
      updateWebPane(event.paneId) {
        $0.agentDriving = event.on
        $0.drivingDomain = event.on ? event.domain : nil
      }
    default: break
    }
  }

  func upsertWebPane(_ pane: Pane) {
    guard pane.mode == .web else { return }
    if webPanes[pane.paneId] == nil { webPanes[pane.paneId] = WebPaneView() }
  }

  func discardWebPane(_ paneId: String) { webPanes.removeValue(forKey: paneId) }

  private func updateWebPane(_ paneId: String, update: (inout WebPaneView) -> Void) {
    guard panes.contains(where: { $0.paneId == paneId && $0.mode == .web }) else { return }
    var view = webPanes[paneId] ?? WebPaneView()
    update(&view)
    webPanes[paneId] = view
  }

  func loadLocalServers() async throws {
    localServers = try await request(.serversLocalList, .object([:]))
      .decoded(ServersLocalListResult.self).servers
  }

  public func refreshLocalServers() {
    perform { try await self.loadLocalServers() }
  }

  public func openWeb(
    workspaceId: String, url: String, newPane: Bool = false, incognito: Bool = false
  ) async throws -> WebOpenResult {
    let result = try await mutation(
      .webOpen,
      WebOpenParams(
        url: url, workspaceId: workspaceId, newPane: newPane, incognito: incognito,
        idempotencyKey: UUID().uuidString))
    let opened = try result.decoded(WebOpenResult.self)
    select(opened.paneId)
    return opened
  }

  public func navigateWeb(
    paneId: String, url: String? = nil, action: WebNavigationAction? = nil
  ) async throws -> WebNavigateResult {
    try await mutation(
      .webNavigate,
      WebNavigateParams(
        paneId: paneId, url: url, action: action, idempotencyKey: UUID().uuidString)
    )
    .decoded(WebNavigateResult.self)
  }

  public func resizeWeb(
    paneId: String, preset: WebViewportPreset? = nil, width: UInt32? = nil,
    height: UInt32? = nil, rotate: Bool = false
  ) async throws -> WebResizeResult {
    try await mutation(
      .webResize,
      WebResizeParams(
        paneId: paneId, preset: preset, width: width, height: height, rotate: rotate,
        idempotencyKey: UUID().uuidString)
    )
    .decoded(WebResizeResult.self)
  }

}
