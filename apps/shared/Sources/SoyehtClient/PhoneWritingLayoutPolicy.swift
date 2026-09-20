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
    guard let state = try? JSONDecoder().decode(Self.self, from: data),
      state.version == currentVersion
    else { return nil }
    return state
  }
}

public enum PhoneWritingStateAction: Equatable, Sendable {
  case toggle(PhoneWritingIdentity)
  case selectTab(PhoneWritingIdentity, PhoneWritingTab)
  case setScreenplayZoom(PhoneWritingIdentity, Double)
}

public struct PhoneWritingViewportInsets: Equatable, Sendable {
  public var top: Double
  public var leading: Double
  public var bottom: Double
  public var trailing: Double

  public init(top: Double = 0, leading: Double = 0, bottom: Double = 0, trailing: Double = 0) {
    self.top = top
    self.leading = leading
    self.bottom = bottom
    self.trailing = trailing
  }
}

public struct PhoneWritingLayoutMetrics: Equatable, Sendable {
  /// Caio's acceptance ruler: the complete layout must work at iPhone mini width.
  public static let minimumExercisedViewportWidth = 375.0

  public var minimumHitTarget: Double
  public var horizontalInset: Double
  public var verticalInset: Double
  public var rowSpacing: Double
  public var maximumScreenplayZoom: Double

  public init(
    minimumHitTarget: Double, horizontalInset: Double, verticalInset: Double,
    rowSpacing: Double, maximumScreenplayZoom: Double
  ) {
    self.minimumHitTarget = minimumHitTarget
    self.horizontalInset = horizontalInset
    self.verticalInset = verticalInset
    self.rowSpacing = rowSpacing
    self.maximumScreenplayZoom = maximumScreenplayZoom
  }

  public static let standard = PhoneWritingLayoutMetrics(
    minimumHitTarget: 44,
    // Leaves 345 pt at the 375 pt ruler: every measured writing floor fits, including
    // the 344 pt screenplay page at its accepted 7.5 pt raster size.
    horizontalInset: 15,
    verticalInset: 12,
    rowSpacing: 12,
    maximumScreenplayZoom: 4)
}

public struct PhoneWritingLayoutRequest: Equatable, Sendable {
  public var tree: PaneLayoutTree?
  public var documents: [PhoneWritingDocumentDescriptor]
  public var selectedPaneId: String?
  /// `nil` means this device has never stored state for this workspace.
  public var localState: PhoneWritingLocalState?
  public var viewport: PaneLayoutSize
  public var safeArea: PhoneWritingViewportInsets
  /// Software keyboard or another bottom obstruction. It changes visible height, not the tree.
  public var obscuredBottom: Double

  public init(
    tree: PaneLayoutTree?, documents: [PhoneWritingDocumentDescriptor],
    selectedPaneId: String? = nil, localState: PhoneWritingLocalState? = nil,
    viewport: PaneLayoutSize,
    safeArea: PhoneWritingViewportInsets = PhoneWritingViewportInsets(),
    obscuredBottom: Double = 0
  ) {
    self.tree = tree
    self.documents = documents
    self.selectedPaneId = selectedPaneId
    self.localState = localState
    self.viewport = viewport
    self.safeArea = safeArea
    self.obscuredBottom = obscuredBottom
  }
}

public struct PhoneWritingLeafLayout: Equatable, Sendable {
  public var identity: PhoneWritingIdentity
  public var name: String
  public var kind: PaneLayoutKind
  public var font: PaneLayoutFontMetric?
  public var fullWidth: Bool
  public var availability: PhoneWritingAvailability
  public var companionAgentPaneId: String?
  public var expanded: Bool
  public var activeTab: PhoneWritingTab
  public var rowRect: PaneLayoutRect
  public var headerRect: PaneLayoutRect
  public var documentTabRect: PaneLayoutRect?
  public var agentTabRect: PaneLayoutRect?
  public var contentRect: PaneLayoutRect?
  /// Whole-page fit before the person's transient pinch multiplier.
  public var screenplayFitScale: Double?
  public var screenplayZoom: Double

  public init(
    identity: PhoneWritingIdentity, name: String, kind: PaneLayoutKind,
    font: PaneLayoutFontMetric?, fullWidth: Bool,
    availability: PhoneWritingAvailability, companionAgentPaneId: String?,
    expanded: Bool, activeTab: PhoneWritingTab, rowRect: PaneLayoutRect,
    headerRect: PaneLayoutRect, documentTabRect: PaneLayoutRect?,
    agentTabRect: PaneLayoutRect?, contentRect: PaneLayoutRect?,
    screenplayFitScale: Double?, screenplayZoom: Double
  ) {
    self.identity = identity
    self.name = name
    self.kind = kind
    self.font = font
    self.fullWidth = fullWidth
    self.availability = availability
    self.companionAgentPaneId = companionAgentPaneId
    self.expanded = expanded
    self.activeTab = activeTab
    self.rowRect = rowRect
    self.headerRect = headerRect
    self.documentTabRect = documentTabRect
    self.agentTabRect = agentTabRect
    self.contentRect = contentRect
    self.screenplayFitScale = screenplayFitScale
    self.screenplayZoom = screenplayZoom
  }
}

public struct PhoneWritingLayoutPlan: Equatable, Sendable {
  public var leaves: [PhoneWritingLeafLayout]
  public var localState: PhoneWritingLocalState
  public var viewportRect: PaneLayoutRect
  public var contentSize: PaneLayoutSize
  public var overflowAxes: Set<PaneLayoutAxis>
  public var isAtLeastMinimumExercisedWidth: Bool

  public init(
    leaves: [PhoneWritingLeafLayout], localState: PhoneWritingLocalState,
    viewportRect: PaneLayoutRect, contentSize: PaneLayoutSize,
    overflowAxes: Set<PaneLayoutAxis>, isAtLeastMinimumExercisedWidth: Bool
  ) {
    self.leaves = leaves
    self.localState = localState
    self.viewportRect = viewportRect
    self.contentSize = contentSize
    self.overflowAxes = overflowAxes
    self.isAtLeastMinimumExercisedWidth = isAtLeastMinimumExercisedWidth
  }
}

/// Pure iPhone shell policy. SwiftUI measures once and renders these rectangles.
public struct PhoneWritingLayoutPolicy: Equatable, Sendable {
  public var metrics: PhoneWritingLayoutMetrics
  public var writingMetrics: WritingLayoutMetrics
  public var floors: [PaneLayoutKind: PaneLayoutSize]

  public init(
    metrics: PhoneWritingLayoutMetrics = .standard,
    writingMetrics: WritingLayoutMetrics = .standard,
    floors: [PaneLayoutKind: PaneLayoutSize]
  ) {
    self.metrics = metrics
    self.writingMetrics = writingMetrics
    self.floors = floors
  }

  public func layout(_ request: PhoneWritingLayoutRequest) -> PhoneWritingLayoutPlan {
    let projected = project(tree: request.tree, documents: request.documents)
    let state = reconciledState(
      request.localState, projected: projected, selectedPaneId: request.selectedPaneId)
    let stateByIdentity = Dictionary(uniqueKeysWithValues: state.panes.map { ($0.identity, $0) })
    let viewport = sanitized(request.viewport)
    let safe = sanitized(request.safeArea)
    let obstruction = finiteNonnegative(request.obscuredBottom)
    let left = safe.leading + finiteNonnegative(metrics.horizontalInset)
    let top = safe.top + finiteNonnegative(metrics.verticalInset)
    let right = safe.trailing + finiteNonnegative(metrics.horizontalInset)
    let bottom = safe.bottom + finiteNonnegative(metrics.verticalInset) + obstruction
    let availableWidth = max(0, viewport.width - left - right)
    let availableHeight = max(0, viewport.height - top - bottom)
    let viewportRect = rect(x: left, y: top, width: availableWidth, height: availableHeight)

    var y = top
    var leaves: [PhoneWritingLeafLayout] = []
    for item in projected {
      let paneState =
        stateByIdentity[item.identity]
        ?? PhoneWritingPaneState(
          identity: item.identity)
      let hasCompanion = item.companionAgentPaneId != nil
      let titleHeight = finiteNonnegative(metrics.minimumHitTarget)
      let tabHeight = hasCompanion ? finiteNonnegative(metrics.minimumHitTarget) : 0
      let headerHeight = titleHeight + tabHeight
      let header = rect(x: left, y: y, width: availableWidth, height: headerHeight)
      let documentTab: PaneLayoutRect?
      let agentTab: PaneLayoutRect?
      if hasCompanion {
        let width = availableWidth / 2
        documentTab = rect(x: left, y: y + titleHeight, width: width, height: tabHeight)
        agentTab = rect(
          x: left + width, y: y + titleHeight,
          width: availableWidth - width, height: tabHeight)
      } else {
        documentTab = nil
        agentTab = nil
      }

      var content: PaneLayoutRect?
      if paneState.expanded {
        let floor = contentFloor(for: item)
        let visibleRemainder = max(0, availableHeight - headerHeight)
        let height = max(floor.height, visibleRemainder)
        content = rect(x: left, y: y + headerHeight, width: availableWidth, height: height)
      }
      let rowHeight = headerHeight + (content?.size.height ?? 0)
      let row = rect(x: left, y: y, width: availableWidth, height: rowHeight)
      let fitScale = screenplayFitScale(for: item, availableWidth: availableWidth)
      leaves.append(
        PhoneWritingLeafLayout(
          identity: item.identity, name: item.name, kind: item.kind,
          font: item.font, fullWidth: item.fullWidth,
          availability: item.availability, companionAgentPaneId: item.companionAgentPaneId,
          expanded: paneState.expanded, activeTab: paneState.activeTab,
          rowRect: row, headerRect: header, documentTabRect: documentTab,
          agentTabRect: agentTab, contentRect: content,
          screenplayFitScale: fitScale, screenplayZoom: paneState.screenplayZoom))
      y += rowHeight + finiteNonnegative(metrics.rowSpacing)
    }
    if !leaves.isEmpty { y -= finiteNonnegative(metrics.rowSpacing) }
    // Keep enough trailing content for the final row to scroll above every bottom
    // obstruction. Shrinking viewportRect alone reports overflow without making
    // that overflow reachable when the ScrollView still has its full physical frame.
    let contentHeight = max(viewport.height, y + bottom)
    let contentSize = PaneLayoutSize(width: viewport.width, height: contentHeight)
    let visibleBottom = viewportRect.origin.y + viewportRect.size.height
    let overflow: Set<PaneLayoutAxis> = y > visibleBottom ? [.vertical] : []
    return PhoneWritingLayoutPlan(
      leaves: leaves, localState: state, viewportRect: viewportRect,
      contentSize: contentSize, overflowAxes: overflow,
      isAtLeastMinimumExercisedWidth:
        viewport.width >= PhoneWritingLayoutMetrics.minimumExercisedViewportWidth)
  }

  public func applying(
    _ action: PhoneWritingStateAction, to plan: PhoneWritingLayoutPlan
  ) -> PhoneWritingLocalState {
    var state = plan.localState
    guard
      let index = state.panes.firstIndex(where: { paneState in
        switch action {
        case .toggle(let identity), .selectTab(let identity, _),
          .setScreenplayZoom(let identity, _):
          paneState.identity == identity
        }
      }), let leaf = plan.leaves.first(where: { $0.identity == state.panes[index].identity })
    else { return state }

    switch action {
    case .toggle:
      state.panes[index].expanded.toggle()
    case .selectTab(_, let tab):
      if tab == .document || leaf.companionAgentPaneId != nil {
        state.panes[index].activeTab = tab
        state.panes[index].expanded = true
      }
    case .setScreenplayZoom(_, let zoom):
      guard leaf.kind == .screenplay, zoom.isFinite else { return state }
      state.panes[index].screenplayZoom = min(
        max(1, zoom), finitePositive(metrics.maximumScreenplayZoom, fallback: 1))
    }
    return state
  }

  private struct ProjectedLeaf: Equatable, Sendable {
    var identity: PhoneWritingIdentity
    var name: String
    var kind: PaneLayoutKind
    var font: PaneLayoutFontMetric?
    var fullWidth: Bool
    var availability: PhoneWritingAvailability
    var companionAgentPaneId: String?
  }

  private func project(
    tree: PaneLayoutTree?, documents: [PhoneWritingDocumentDescriptor]
  ) -> [ProjectedLeaf] {
    guard let tree else { return [] }
    let grouped = Dictionary(grouping: documents, by: \PhoneWritingDocumentDescriptor.paneId)
    var result: [ProjectedLeaf] = []
    var seen = Set<String>()

    func append(_ leaf: PaneLayoutLeaf, companionAgentPaneId: String?) {
      guard isWriting(leaf.kind), seen.insert(leaf.paneId).inserted else { return }
      let matches = grouped[leaf.paneId] ?? []
      let descriptor = matches.count == 1 ? matches[0] : nil
      let valid =
        descriptor.map {
          $0.kind == leaf.kind && !$0.fileId.isEmpty && validFont($0.font)
        } == true
      let availability: PhoneWritingAvailability
      if matches.isEmpty {
        availability = .missingDocument
      } else if valid {
        availability = .ready
      } else {
        availability = .inconsistentDocument
      }
      result.append(
        ProjectedLeaf(
          identity: PhoneWritingIdentity(
            paneId: leaf.paneId, fileId: valid ? descriptor?.fileId : nil),
          name: descriptor?.name ?? leaf.paneId, kind: leaf.kind,
          font: valid ? descriptor?.font : nil,
          fullWidth: valid && leaf.kind == .text ? descriptor?.fullWidth == true : false,
          availability: availability,
          companionAgentPaneId: companionAgentPaneId))
    }

    func visit(_ node: PaneLayoutTree) {
      switch node {
      case .leaf(let leaf):
        append(leaf, companionAgentPaneId: nil)
      case .split(let split):
        if let pair = validCompanion(in: split) {
          append(pair.writing, companionAgentPaneId: pair.agent.paneId)
          return
        }
        for child in split.children { visit(child.tree) }
      }
    }
    visit(tree)
    return result
  }

  private func validCompanion(
    in split: PaneLayoutSplit
  ) -> (writing: PaneLayoutLeaf, agent: PaneLayoutLeaf)? {
    guard let companion = split.companion, split.children.count == 2 else { return nil }
    let leaves = split.children.compactMap { branch -> PaneLayoutLeaf? in
      guard case .leaf(let leaf) = branch.tree else { return nil }
      return leaf
    }
    guard leaves.count == 2,
      Set(leaves.map(\.paneId))
        == Set([companion.writingPaneId, companion.agentPaneId]),
      let writing = leaves.first(where: { $0.paneId == companion.writingPaneId }),
      isWriting(writing.kind),
      let agent = leaves.first(where: { $0.paneId == companion.agentPaneId }),
      agent.kind == .agent
    else { return nil }
    return (writing, agent)
  }

  private func reconciledState(
    _ stored: PhoneWritingLocalState?, projected: [ProjectedLeaf], selectedPaneId: String?
  ) -> PhoneWritingLocalState {
    let validStored = stored?.version == PhoneWritingLocalState.currentVersion ? stored : nil
    let existing: [PhoneWritingIdentity: PhoneWritingPaneState]
    if let validStored {
      existing = Dictionary(
        validStored.panes.reversed().map { ($0.identity, $0) },
        uniquingKeysWith: { first, _ in first })
    } else {
      existing = [:]
    }
    let initialPaneId: String? = {
      guard validStored == nil else { return nil }
      if let selectedPaneId,
        let selected = projected.first(where: {
          $0.identity.paneId == selectedPaneId || $0.companionAgentPaneId == selectedPaneId
        })
      {
        return selected.identity.paneId
      }
      return projected.first?.identity.paneId
    }()

    let panes = projected.map { item -> PhoneWritingPaneState in
      var pane =
        existing[item.identity]
        ?? PhoneWritingPaneState(
          identity: item.identity,
          expanded: item.identity.paneId == initialPaneId,
          activeTab:
            item.companionAgentPaneId == selectedPaneId && item.identity.paneId == initialPaneId
            ? .agent : .document)
      if item.companionAgentPaneId == nil { pane.activeTab = .document }
      if item.kind != .screenplay {
        pane.screenplayZoom = 1
      } else {
        pane.screenplayZoom = min(
          max(1, finitePositive(pane.screenplayZoom, fallback: 1)),
          finitePositive(metrics.maximumScreenplayZoom, fallback: 1))
      }
      return pane
    }
    return PhoneWritingLocalState(panes: panes)
  }

  private func contentFloor(for item: ProjectedLeaf) -> PaneLayoutSize {
    let supplied = floors[item.kind] ?? PaneLayoutSize(width: 0, height: 0)
    let measured =
      item.font.flatMap { writingMetrics.surfaceFloor(kind: item.kind, font: $0) }
      ?? supplied
    var width = max(supplied.width, measured.width)
    var height = max(supplied.height, measured.height) - writingMetrics.headerHeight
    if item.companionAgentPaneId != nil, let agent = floors[.agent] {
      width = max(width, agent.width)
      height = max(height, agent.height)
    }
    return PaneLayoutSize(width: max(0, width), height: max(0, height))
  }

  private func screenplayFitScale(
    for item: ProjectedLeaf, availableWidth: Double
  ) -> Double? {
    guard item.kind == .screenplay, let font = item.font else { return nil }
    let baseWidth =
      writingMetrics.screenplayColumns * font.characterWidth
      + 2 * writingMetrics.screenplayPageHorizontalInset
    guard baseWidth.isFinite, baseWidth > 0 else { return nil }
    return min(1, max(writingMetrics.screenplayMinimumScale, availableWidth / baseWidth))
  }

  private func isWriting(_ kind: PaneLayoutKind) -> Bool {
    [.text, .screenplay, .presentation].contains(kind)
  }

  private func validFont(_ font: PaneLayoutFontMetric) -> Bool {
    font.characterWidth.isFinite && font.characterWidth > 0
      && font.lineHeight.isFinite && font.lineHeight > 0
      && font.pointSize.isFinite && font.pointSize > 0
  }

  private func sanitized(_ size: PaneLayoutSize) -> PaneLayoutSize {
    PaneLayoutSize(width: finiteNonnegative(size.width), height: finiteNonnegative(size.height))
  }

  private func sanitized(_ insets: PhoneWritingViewportInsets) -> PhoneWritingViewportInsets {
    PhoneWritingViewportInsets(
      top: finiteNonnegative(insets.top), leading: finiteNonnegative(insets.leading),
      bottom: finiteNonnegative(insets.bottom), trailing: finiteNonnegative(insets.trailing))
  }

  private func finiteNonnegative(_ value: Double) -> Double {
    value.isFinite ? max(0, value) : 0
  }

  private func finitePositive(_ value: Double, fallback: Double) -> Double {
    value.isFinite && value > 0 ? value : fallback
  }

  private func rect(
    x: Double, y: Double, width: Double, height: Double
  ) -> PaneLayoutRect {
    PaneLayoutRect(
      origin: PaneLayoutPoint(x: x, y: y),
      size: PaneLayoutSize(width: max(0, width), height: max(0, height)))
  }
}
