import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// Point sizes of the top bar's SF Symbols, kept with the bar that draws them.
private enum WorkspaceTabBarGlyph {
  static let toggle: CGFloat = 15
  static let newPane: CGFloat = 14
  static let tabFolder: CGFloat = 14
  static let tabClose: CGFloat = 9
  static let chevron: CGFloat = 11
}

private struct WorkspaceTabFramePreferenceKey: PreferenceKey {
  static let defaultValue: [String: CGRect] = [:]

  static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
    value.merge(nextValue(), uniquingKeysWith: { _, new in new })
  }
}

private struct WorkspaceTabStripMeasurement: Equatable {
  /// Tab frames in scroll-content coordinates.
  var items: [WorkspaceTabStripItem]
  var viewportWidth: Double
  var contentWidth: Double

  static let empty = WorkspaceTabStripMeasurement(
    items: [], viewportWidth: 0, contentWidth: 0)
}

/// The workspace tabs above the panes. The strip scrolls sideways like any Apple scroll view:
/// trackpad swipes, Shift+wheel and a plain mouse wheel move it, a thin overlay indicator shows
/// while it moves, and an edge fade with a chevron marks each side that hides tabs. The active
/// tab is scrolled into view whenever it changes; scrolled away by hand, it stays reachable as a
/// sticky proxy at the edge it left from. The "+N" button lists every hidden workspace.
struct WorkspaceTopBar: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  let onSelectWorkspace: (String) -> Void
  let onCloseWorkspace: (String) -> Void
  let onNewPane: () -> Void
  let onTasks: () -> Void
  let onToggleAppBar: () -> Void

  @State private var measurement = WorkspaceTabStripMeasurement.empty
  @State private var scrollOffset = 0.0
  @State private var scroller = HorizontalScrollController()
  @State private var plan = WorkspaceTabStripPlan(
    fullyVisibleWorkspaceIds: [], hiddenWorkspaceIds: [], sticky: nil)
  @State private var showsOverflowControl = false
  @State private var overflowPresented = false
  @FocusState private var focusedWorkspaceId: String?

  private static let contentSpace = "workspace-tab-strip-content"

  var body: some View {
    ScrollViewReader { proxy in
      HStack(spacing: MacTheme.Space.x10) {
        Color.clear.frame(width: 52, height: MacPaneLayoutMetrics.topBarControlHeight)
        Button(action: onToggleAppBar) {
          Image(systemName: "rectangle.grid.2x2")
            .font(.system(size: WorkspaceTabBarGlyph.toggle, weight: .semibold))
            .frame(
              width: MacPaneLayoutMetrics.topBarControlHeight,
              height: MacPaneLayoutMetrics.topBarControlHeight
            )
            .neoRaised(radius: MacTheme.Radius.control)
        }
        .help("Toggle app bar")
        .accessibilityLabel("Toggle app bar")
        .accessibilityIdentifier("toggle-app-bar")

        workspaceStrip(proxy)

        if showsOverflowControl, plan.hiddenCount > 0 {
          Button("+\(plan.hiddenCount)") { overflowPresented.toggle() }
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
            .foregroundStyle(MacTheme.accent)
            .frame(
              width: MacPaneLayoutMetrics.workspaceOverflowButtonWidth,
              height: MacPaneLayoutMetrics.topBarControlHeight
            )
            .neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
            .help("Show \(plan.hiddenCount) more workspaces")
            .accessibilityLabel("\(plan.hiddenCount) more workspaces")
            .accessibilityIdentifier("workspace-tabs-overflow")
            .popover(isPresented: $overflowPresented, arrowEdge: .bottom) {
              WorkspaceOverflowList(
                workspaces: hiddenWorkspaces, attention: client.agentAttention,
                onSelect: { workspaceId in
                  overflowPresented = false
                  onSelectWorkspace(workspaceId)
                  reveal(workspaceId, edge: nil, using: proxy)
                })
            }
        }

        Button(action: onNewPane) {
          Image(systemName: "plus")
            .font(.system(size: WorkspaceTabBarGlyph.newPane, weight: .semibold))
            .frame(
              width: MacPaneLayoutMetrics.topBarControlHeight,
              height: MacPaneLayoutMetrics.topBarControlHeight
            ).neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
        }
        .help("Open a terminal or agent pane").disabled(!client.connected)
        .accessibilityLabel("Open a terminal or agent pane")
        .accessibilityIdentifier("new-pane")
        if !client.connected {
          HStack(spacing: MacTheme.Space.x6) {
            Circle().fill(MacTheme.warning).frame(width: 7, height: 7)
            Text("Reconnecting")
              .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .semibold))
              .foregroundStyle(MacTheme.secondary)
          }
        }
      }
      .frame(height: MacPaneLayoutMetrics.topBarHeight)
      .onChange(of: workspaceId) { _, selectedId in
        let nextPlan = planned(from: measurement, activeWorkspaceId: selectedId)
        plan = nextPlan
        guard let selectedId, !nextPlan.fullyVisibleWorkspaceIds.contains(selectedId) else {
          return
        }
        reveal(selectedId, edge: nextPlan.sticky?.edge, using: proxy)
      }
      .onChange(of: client.workspaces.map(\.workspaceId)) { _, workspaceIds in
        let liveIds = Set(workspaceIds)
        if !plan.hiddenWorkspaceIds.allSatisfy(liveIds.contains) {
          updatePlan(from: measurement)
        }
      }
    }
  }

  private var hiddenWorkspaces: [Workspace] {
    let hidden = Set(plan.hiddenWorkspaceIds)
    return client.workspaces.filter { hidden.contains($0.workspaceId) }
  }

  private var viewport: WorkspaceTabStripViewport {
    WorkspaceTabStripViewport(offset: scrollOffset, width: measurement.viewportWidth)
  }

  private var overflow: WorkspaceTabStripOverflow {
    WorkspaceTabStripPolicy.overflow(contentWidth: measurement.contentWidth, viewport: viewport)
  }

  private func workspaceStrip(_ proxy: ScrollViewProxy) -> some View {
    ScrollView(.horizontal, showsIndicators: true) {
      HStack(spacing: MacTheme.Space.x10) {
        ForEach(client.workspaces, id: \.workspaceId) { workspace in
          WorkspaceTab(
            workspaceId: workspace.workspaceId, name: workspace.name,
            selected: workspace.workspaceId == workspaceId,
            waitingCount: client.agentAttention.waitingCount(in: workspace.workspaceId),
            isWorking: client.agentAttention.hasWorkingAgent(in: workspace.workspaceId),
            keyboardFocusable: plan.sticky?.workspaceId != workspace.workspaceId,
            focusedWorkspaceId: $focusedWorkspaceId,
            action: { onSelectWorkspace(workspace.workspaceId) },
            onClose: { onCloseWorkspace(workspace.workspaceId) }
          )
          .opacity(plan.sticky?.workspaceId == workspace.workspaceId ? 0 : 1)
          .allowsHitTesting(plan.sticky?.workspaceId != workspace.workspaceId)
          .accessibilityHidden(plan.sticky?.workspaceId == workspace.workspaceId)
          .id(workspace.workspaceId)
          .background {
            GeometryReader { geometry in
              Color.clear.preference(
                key: WorkspaceTabFramePreferenceKey.self,
                value: [workspace.workspaceId: geometry.frame(in: .named(Self.contentSpace))])
            }
          }
        }
      }
      .padding(MacPaneLayoutMetrics.workspaceTabShadowClearance)
      .coordinateSpace(name: Self.contentSpace)
      .background {
        HorizontalScrollBridge(controller: scroller) { offset in
          guard abs(offset - scrollOffset) > 0.25 else { return }
          scrollOffset = offset
          plan = planned(from: measurement, activeWorkspaceId: workspaceId)
        }
      }
    }
    .accessibilityIdentifier("workspace-tab-strip")
    .frame(maxWidth: .infinity)
    .frame(height: MacPaneLayoutMetrics.workspaceTabViewportHeight)
    .clipped()
    .overlayPreferenceValue(WorkspaceTabFramePreferenceKey.self) { frames in
      GeometryReader { geometry in
        let items = client.workspaces.compactMap { workspace -> WorkspaceTabStripItem? in
          guard let frame = frames[workspace.workspaceId] else { return nil }
          return WorkspaceTabStripItem(
            workspaceId: workspace.workspaceId,
            minX: Double(frame.minX), width: Double(frame.width))
        }
        Color.clear.preference(
          key: WorkspaceTabMeasurementPreferenceKey.self,
          value: WorkspaceTabStripMeasurement(
            items: items, viewportWidth: Double(geometry.size.width),
            contentWidth: contentWidth(for: items)))
      }
    }
    .onPreferenceChange(WorkspaceTabMeasurementPreferenceKey.self) { updatePlan(from: $0) }
    .onMoveCommand { direction in moveWorkspaceFocus(direction, using: proxy) }
    .overlay { workspaceTabEdges }
    .overlay(alignment: plan.sticky?.edge == .leading ? .leading : .trailing) {
      if let sticky = plan.sticky,
        let workspace = client.workspaces.first(where: { $0.workspaceId == sticky.workspaceId })
      {
        ZStack(alignment: sticky.edge == .leading ? .leading : .trailing) {
          workspaceStickyBackdrop(
            edge: sticky.edge, width: stickyOcclusionWidth(for: sticky, in: measurement))
          WorkspaceTab(
            workspaceId: workspace.workspaceId, name: workspace.name, selected: true,
            waitingCount: client.agentAttention.waitingCount(in: workspace.workspaceId),
            isWorking: client.agentAttention.hasWorkingAgent(in: workspace.workspaceId),
            keyboardFocusable: false, focusedWorkspaceId: nil,
            action: { reveal(workspace.workspaceId, edge: sticky.edge, using: proxy) },
            onClose: { onCloseWorkspace(workspace.workspaceId) },
            identifierPrefix: "workspace-tab-sticky"
          )
          .frame(width: stickyTabWidth(for: sticky, in: measurement))
          .padding(.horizontal, MacPaneLayoutMetrics.workspaceTabShadowClearance)
        }
        .frame(
          width: stickyOcclusionWidth(for: sticky, in: measurement),
          height: MacPaneLayoutMetrics.workspaceTabViewportHeight,
          alignment: sticky.edge == .leading ? .leading : .trailing
        )
        .transition(.opacity)
      }
    }
    .animation(.easeInOut(duration: 0.16), value: plan.sticky)
    .animation(.easeInOut(duration: 0.16), value: overflow)
  }

  /// A fade and a chevron on each edge that hides tabs, unless the sticky proxy covers it.
  private var workspaceTabEdges: some View {
    HStack(spacing: 0) {
      if overflow.hiddenLeading, plan.sticky?.edge != .leading {
        edge(.leading)
      }
      Spacer(minLength: 0)
      if overflow.hiddenTrailing, plan.sticky?.edge != .trailing {
        edge(.trailing)
      }
    }
    .frame(height: MacPaneLayoutMetrics.workspaceTabViewportHeight)
  }

  private func edge(_ edge: WorkspaceTabStickyEdge) -> some View {
    let leading = edge == .leading
    return ZStack {
      LinearGradient(
        colors: leading
          ? [MacTheme.canvas, MacTheme.canvas.opacity(0)]
          : [MacTheme.canvas.opacity(0), MacTheme.canvas],
        startPoint: .leading, endPoint: .trailing
      )
      .allowsHitTesting(false)
      Button {
        page(toward: edge)
      } label: {
        Image(systemName: leading ? "chevron.left" : "chevron.right")
          .font(.system(size: WorkspaceTabBarGlyph.chevron, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
          .frame(
            width: MacPaneLayoutMetrics.workspaceTabFadeWidth,
            height: MacPaneLayoutMetrics.workspaceTabHeight
          )
          .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .frame(maxWidth: .infinity, alignment: leading ? .leading : .trailing)
      .help(leading ? "Scroll to earlier workspaces" : "Scroll to later workspaces")
      .accessibilityLabel(leading ? "Scroll to earlier workspaces" : "Scroll to later workspaces")
      .accessibilityIdentifier(
        leading ? "workspace-tabs-scroll-leading" : "workspace-tabs-scroll-trailing")
    }
    .frame(width: 2 * MacPaneLayoutMetrics.workspaceTabFadeWidth)
    .transition(.opacity)
  }

  private func page(toward edge: WorkspaceTabStickyEdge) {
    scroller.scroll(
      to: WorkspaceTabStripPolicy.pageOffset(
        toward: edge, viewport: viewport, contentWidth: measurement.contentWidth))
  }

  private func updatePlan(from nextMeasurement: WorkspaceTabStripMeasurement) {
    measurement = nextMeasurement
    let overflowReservation =
      MacPaneLayoutMetrics.workspaceOverflowButtonWidth + Double(MacTheme.Space.x10)
    // The overflow control takes room from the strip; measure against the width the strip
    // would have without it so showing it cannot hide it again.
    let widthWithoutOverflow =
      nextMeasurement.viewportWidth
      + (showsOverflowControl ? overflowReservation : 0)
    showsOverflowControl = WorkspaceTabStripPolicy.overflows(
      contentWidth: nextMeasurement.contentWidth, availableWidth: widthWithoutOverflow)
    plan = planned(from: nextMeasurement, activeWorkspaceId: workspaceId)
  }

  private func contentWidth(for items: [WorkspaceTabStripItem]) -> Double {
    guard let maximum = items.map(\.maxX).max() else { return 0 }
    return maximum + MacPaneLayoutMetrics.workspaceTabShadowClearance
  }

  private func planned(
    from measurement: WorkspaceTabStripMeasurement, activeWorkspaceId: String?
  ) -> WorkspaceTabStripPlan {
    let viewport = WorkspaceTabStripViewport(
      offset: scrollOffset, width: measurement.viewportWidth)
    let initial = WorkspaceTabStripPolicy.plan(
      items: measurement.items, activeWorkspaceId: activeWorkspaceId,
      viewport: viewport)
    let occlusion = initial.sticky.map { stickyOcclusionWidth(for: $0, in: measurement) } ?? 0
    return WorkspaceTabStripPolicy.plan(
      items: measurement.items, activeWorkspaceId: activeWorkspaceId,
      viewport: viewport, stickyOcclusionWidth: occlusion)
  }

  private func stickyOcclusionWidth(
    for sticky: WorkspaceTabStickyPresentation, in measurement: WorkspaceTabStripMeasurement
  ) -> Double {
    let clearance = MacPaneLayoutMetrics.workspaceTabShadowClearance
    let tabWidth = stickyTabWidth(for: sticky, in: measurement)
    let footprint = tabWidth + 2 * clearance + Double(MacTheme.Space.x10)
    return min(measurement.viewportWidth, footprint)
  }

  private func stickyTabWidth(
    for sticky: WorkspaceTabStickyPresentation, in measurement: WorkspaceTabStripMeasurement
  ) -> Double {
    let measured =
      measurement.items.first(where: { $0.workspaceId == sticky.workspaceId })?.width
      ?? MacPaneLayoutMetrics.workspaceTabMaximumWidth
    return min(
      measured,
      max(
        0,
        measurement.viewportWidth - 2 * MacPaneLayoutMetrics.workspaceTabShadowClearance))
  }

  @ViewBuilder private func workspaceStickyBackdrop(
    edge: WorkspaceTabStickyEdge, width: Double
  ) -> some View {
    let fadeWidth = min(width, MacPaneLayoutMetrics.workspaceTabFadeWidth)
    let opaqueWidth = max(0, width - fadeWidth)
    HStack(spacing: 0) {
      if edge == .leading {
        MacTheme.canvas.frame(width: opaqueWidth)
        LinearGradient(
          colors: [MacTheme.canvas, MacTheme.canvas.opacity(0)],
          startPoint: .leading, endPoint: .trailing
        )
        .frame(width: fadeWidth)
      } else {
        LinearGradient(
          colors: [MacTheme.canvas.opacity(0), MacTheme.canvas],
          startPoint: .leading, endPoint: .trailing
        )
        .frame(width: fadeWidth)
        MacTheme.canvas.frame(width: opaqueWidth)
      }
    }
    .frame(width: width, height: MacPaneLayoutMetrics.workspaceTabViewportHeight)
    .allowsHitTesting(false)
  }

  private func moveWorkspaceFocus(
    _ move: MoveCommandDirection, using proxy: ScrollViewProxy
  ) {
    let direction: WorkspaceTabFocusDirection
    let edge: WorkspaceTabStickyEdge
    switch move {
    case .left:
      direction = .previous
      edge = .leading
    case .right:
      direction = .next
      edge = .trailing
    default:
      return
    }
    let orderedIds = client.workspaces.map(\.workspaceId)
    guard
      let neighbor = WorkspaceTabStripPolicy.focusNeighbor(
        of: focusedWorkspaceId ?? workspaceId, direction: direction,
        orderedWorkspaceIds: orderedIds)
    else { return }
    focusedWorkspaceId = neighbor
    reveal(neighbor, edge: edge, using: proxy)
  }

  /// Scrolls the smallest distance that shows the whole tab. A tab not measured yet (a
  /// workspace created a moment ago) is revealed by its identifier instead.
  private func reveal(
    _ workspaceId: String, edge: WorkspaceTabStickyEdge?, using proxy: ScrollViewProxy
  ) {
    if measurement.items.contains(where: { $0.workspaceId == workspaceId }) {
      guard
        let offset = WorkspaceTabStripPolicy.revealOffset(
          workspaceId: workspaceId, items: measurement.items, viewport: viewport,
          contentWidth: measurement.contentWidth)
      else { return }
      if scroller.scroll(to: offset) { return }
    }
    let anchor: UnitPoint
    switch edge {
    case .leading: anchor = .leading
    case .trailing: anchor = .trailing
    case nil: anchor = .center
    }
    withAnimation(.easeInOut(duration: 0.18)) {
      proxy.scrollTo(workspaceId, anchor: anchor)
    }
  }
}

private struct WorkspaceTabMeasurementPreferenceKey: PreferenceKey {
  static let defaultValue = WorkspaceTabStripMeasurement.empty

  static func reduce(
    value: inout WorkspaceTabStripMeasurement,
    nextValue: () -> WorkspaceTabStripMeasurement
  ) {
    value = nextValue()
  }
}

private struct WorkspaceTab: View {
  let workspaceId: String
  let name: String
  let selected: Bool
  let waitingCount: Int
  let isWorking: Bool
  let keyboardFocusable: Bool
  let focusedWorkspaceId: FocusState<String?>.Binding?
  let action: () -> Void
  let onClose: () -> Void
  var identifierPrefix = "workspace-tab"

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Button(action: action) {
        HStack(spacing: MacTheme.Space.x8) {
          Image(systemName: "folder")
            .font(.system(size: WorkspaceTabBarGlyph.tabFolder, weight: .medium))
            .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
          Text(name).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
            .foregroundStyle(selected ? MacTheme.accent : MacTheme.ink).lineLimit(1)
          if waitingCount > 0 {
            AgentAttentionBadge(count: waitingCount)
          } else if isWorking {
            AgentWorkingDot()
          }
        }
      }
      .modifier(
        WorkspaceTabKeyboardFocus(
          workspaceId: workspaceId, enabled: keyboardFocusable,
          focusedWorkspaceId: focusedWorkspaceId)
      )
      .onKeyPress(.return) {
        guard keyboardFocusable else { return .ignored }
        action()
        return .handled
      }
      .help(name)
      .accessibilityLabel(
        waitingCount > 0
          ? "\(name), \(AgentAttentionPolicy.waitingLabel(count: waitingCount))"
          : isWorking ? "\(name), agents working" : name
      )
      .accessibilityAddTraits(selected ? .isSelected : [])
      .accessibilityIdentifier("\(identifierPrefix)-\(workspaceId)")

      Button(action: onClose) {
        Image(systemName: "xmark")
          .font(.system(size: WorkspaceTabBarGlyph.tabClose, weight: .semibold))
          .foregroundStyle(MacTheme.muted)
      }
      .focusable(false)
      .help("Close \(name)")
      .accessibilityLabel("Close workspace \(name)")
      .accessibilityIdentifier("\(identifierPrefix)-close-\(workspaceId)")
    }
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: MacPaneLayoutMetrics.workspaceTabHeight)
    .frame(maxWidth: MacPaneLayoutMetrics.workspaceTabMaximumWidth)
    .modifier(WorkspaceTabSurface(selected: selected))
  }
}

private struct WorkspaceTabKeyboardFocus: ViewModifier {
  let workspaceId: String
  let enabled: Bool
  let focusedWorkspaceId: FocusState<String?>.Binding?

  @ViewBuilder func body(content: Content) -> some View {
    if enabled, let focusedWorkspaceId {
      content
        .focusable(true)
        .focused(focusedWorkspaceId, equals: workspaceId)
    } else {
      content.focusable(false)
    }
  }
}

private struct WorkspaceTabSurface: ViewModifier {
  let selected: Bool

  func body(content: Content) -> some View {
    if selected {
      content.neoWell(radius: MacTheme.Radius.pill)
    } else {
      content.neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
    }
  }
}

/// Scrolls a strip's AppKit scroll view to an exact offset. SwiftUI's scroll proxy only knows
/// identifiers and anchors, which cannot express "the smallest distance" or "one page".
@MainActor final class HorizontalScrollController {
  fileprivate weak var scrollView: NSScrollView?

  /// Animates the content to `offset`. Returns false while the strip is not on screen.
  @discardableResult func scroll(to offset: Double) -> Bool {
    guard let scrollView else { return false }
    let clipView = scrollView.contentView
    let origin = NSPoint(x: offset, y: clipView.bounds.origin.y)
    NSAnimationContext.runAnimationGroup { context in
      context.duration = 0.18
      context.allowsImplicitAnimation = true
      clipView.animator().setBoundsOrigin(origin)
    }
    scrollView.reflectScrolledClipView(clipView)
    return true
  }
}

/// Placed inside a horizontal SwiftUI `ScrollView`, gives its AppKit scroll view the Apple
/// behaviour of a sideways strip: the thin overlay indicator that shows only while scrolling
/// (AppKit otherwise falls back to a permanent scroller when a mouse is attached), a vertical
/// mouse wheel that scrolls sideways, and the live content offset.
private struct HorizontalScrollBridge: NSViewRepresentable {
  let controller: HorizontalScrollController
  let onOffsetChange: (Double) -> Void

  func makeNSView(context: Context) -> HorizontalScrollBridgeView {
    let view = HorizontalScrollBridgeView()
    update(view)
    return view
  }

  func updateNSView(_ view: HorizontalScrollBridgeView, context: Context) { update(view) }

  private func update(_ view: HorizontalScrollBridgeView) {
    view.controller = controller
    view.onOffsetChange = onOffsetChange
    view.attachSoon()
  }
}

private final class HorizontalScrollBridgeView: NSView {
  weak var controller: HorizontalScrollController?
  var onOffsetChange: ((Double) -> Void)?
  private weak var attached: NSScrollView?
  private var wheelMonitor: Any?
  private var attachPending = false

  override func hitTest(_ point: NSPoint) -> NSView? { nil }

  override func viewDidMoveToWindow() {
    super.viewDidMoveToWindow()
    if window == nil { detach() } else { attachSoon() }
  }

  /// SwiftUI builds the scroll view around this marker after the marker itself, so the
  /// lookup waits one run-loop turn.
  func attachSoon() {
    guard !attachPending else { return }
    attachPending = true
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.attachPending = false
      self.attach()
    }
  }

  private func attach() {
    guard window != nil, let scrollView = enclosingScrollView else { return }
    applyOverlayStyle(to: scrollView)
    controller?.scrollView = scrollView
    guard attached !== scrollView else { return }
    detach()
    attached = scrollView
    controller?.scrollView = scrollView
    let clipView = scrollView.contentView
    clipView.postsBoundsChangedNotifications = true
    NotificationCenter.default.addObserver(
      self, selector: #selector(boundsChanged), name: NSView.boundsDidChangeNotification,
      object: clipView)
    // AppKit drops the overlay style when the system scroll bar preference changes.
    NotificationCenter.default.addObserver(
      self, selector: #selector(preferredStyleChanged),
      name: NSScroller.preferredScrollerStyleDidChangeNotification, object: nil)
    wheelMonitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) {
      [weak self] event in
      self?.sideways(event) ?? event
    }
    boundsChanged()
  }

  private func detach() {
    NotificationCenter.default.removeObserver(self)
    if let wheelMonitor { NSEvent.removeMonitor(wheelMonitor) }
    wheelMonitor = nil
    attached = nil
  }

  private func applyOverlayStyle(to scrollView: NSScrollView) {
    if scrollView.scrollerStyle != .overlay { scrollView.scrollerStyle = .overlay }
    if !scrollView.autohidesScrollers { scrollView.autohidesScrollers = true }
    if scrollView.hasVerticalScroller { scrollView.hasVerticalScroller = false }
  }

  @objc private func boundsChanged() {
    guard let attached else { return }
    onOffsetChange?(Double(attached.contentView.bounds.origin.x))
  }

  @objc private func preferredStyleChanged() {
    if let attached { applyOverlayStyle(to: attached) }
  }

  /// Turns a vertical mouse-wheel event over the strip into the same event on the horizontal
  /// axis, so AppKit scrolls it with its own acceleration and elasticity.
  private func sideways(_ event: NSEvent) -> NSEvent {
    guard let attached, event.window === attached.window,
      attached.bounds.contains(attached.convert(event.locationInWindow, from: nil)),
      WorkspaceTabStripPolicy.wheelScrollsHorizontally(
        deltaX: Double(event.scrollingDeltaX), deltaY: Double(event.scrollingDeltaY),
        hasPreciseDeltas: event.hasPreciseScrollingDeltas),
      let converted = event.cgEvent?.copy()
    else { return event }
    let integerAxes: [(CGEventField, CGEventField)] = [
      (.scrollWheelEventDeltaAxis1, .scrollWheelEventDeltaAxis2),
      (.scrollWheelEventPointDeltaAxis1, .scrollWheelEventPointDeltaAxis2),
    ]
    for (vertical, horizontal) in integerAxes {
      converted.setIntegerValueField(
        horizontal, value: converted.getIntegerValueField(vertical))
      converted.setIntegerValueField(vertical, value: 0)
    }
    let vertical = CGEventField.scrollWheelEventFixedPtDeltaAxis1
    let horizontal = CGEventField.scrollWheelEventFixedPtDeltaAxis2
    converted.setDoubleValueField(horizontal, value: converted.getDoubleValueField(vertical))
    converted.setDoubleValueField(vertical, value: 0)
    return NSEvent(cgEvent: converted) ?? event
  }
}

private struct WorkspaceOverflowList: View {
  let workspaces: [Workspace]
  let attention: AgentAttentionSummary
  let onSelect: (String) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      Text("More workspaces")
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      ScrollView {
        LazyVStack(alignment: .leading, spacing: MacTheme.Space.x6) {
          ForEach(workspaces, id: \.workspaceId) { workspace in
            Button {
              onSelect(workspace.workspaceId)
            } label: {
              HStack(alignment: .top, spacing: MacTheme.Space.x8) {
                Image(systemName: "folder")
                  .foregroundStyle(MacTheme.secondary)
                Text(workspace.name)
                  .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
                  .foregroundStyle(MacTheme.ink)
                  .multilineTextAlignment(.leading)
                  .fixedSize(horizontal: false, vertical: true)
                if attention.waitingCount(in: workspace.workspaceId) > 0 {
                  AgentAttentionBadge(count: attention.waitingCount(in: workspace.workspaceId))
                } else if attention.hasWorkingAgent(in: workspace.workspaceId) {
                  AgentWorkingDot()
                }
                Spacer(minLength: 0)
              }
              .padding(.horizontal, MacTheme.Space.x10)
              .padding(.vertical, MacTheme.Space.x8)
              .contentShape(Rectangle())
            }
            .help(workspace.name)
            .accessibilityLabel(
              attention.waitingCount(in: workspace.workspaceId) > 0
                ? "\(workspace.name), \(AgentAttentionPolicy.waitingLabel(count: attention.waitingCount(in: workspace.workspaceId)))"
                : attention.hasWorkingAgent(in: workspace.workspaceId)
                  ? "\(workspace.name), agents working" : workspace.name
            )
            .accessibilityIdentifier("workspace-overflow-\(workspace.workspaceId)")
          }
        }
      }
      .frame(maxHeight: MacPaneLayoutMetrics.workspaceOverflowPopoverMaximumHeight)
    }
    .padding(MacTheme.Space.x14)
    .frame(width: MacPaneLayoutMetrics.workspaceOverflowPopoverWidth)
    .background(MacTheme.canvas)
  }
}
