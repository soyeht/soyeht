import AppKit
import Observation
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The document one writing pane shows and the actions every writing pane takes on it. The
/// panes are thin views: what is in the menu, how long the chrome fades and when closing asks
/// first are `WritingChromePolicy` and `WritingChromeMetrics` in `SoyehtClient`.
@MainActor struct WritingPaneContext {
  let client: EngineClient
  let pane: Pane

  var writing: WritingDocumentView? {
    client.writingDocuments.values.first { $0.document.file.paneId == pane.paneId }
  }

  var file: EditorFileView? {
    guard let fileId = writing?.document.file.fileId else { return nil }
    return client.editorFiles[fileId]
  }

  var loadKey: String { writing?.loadIdentity ?? "missing" }

  var documentFileName: String {
    guard let path = writing?.document.file.path else { return pane.name }
    return URL(fileURLWithPath: path).lastPathComponent
  }

  /// Loads the document's first viewport. A pane that only needs the text once passes
  /// `onlyWithoutViewport`.
  func loadIfNeeded(onlyWithoutViewport: Bool = false) async {
    guard let writing, !onlyWithoutViewport || file?.viewport == nil else { return }
    do { try await client.loadWritingPane(writing.document.file.paneId) } catch {
      client.problem = error.localizedDescription
    }
  }

  func setPreferences(font: WritingFont? = nil, focusMode: Bool? = nil, fullWidth: Bool? = nil) {
    guard let fileId = writing?.document.file.fileId else { return }
    client.perform {
      _ = try await client.setWritingPreferences(
        fileId: fileId, font: font, focusMode: focusMode, fullWidth: fullWidth)
    }
  }

  /// Loads the viewport that holds `line` when the one on screen does not.
  func revealLine(_ line: UInt32) {
    guard let fileId = writing?.document.file.fileId, let viewport = file?.viewport,
      line < viewport.fromLine || line >= viewport.toLine
    else { return }
    client.perform { try await client.loadEditorViewport(fileId: fileId, fromLine: line) }
  }
}

/// What the chrome of one writing pane shows right now: how far it has faded and whether a
/// close waits for the person's answer.
@MainActor @Observable final class WritingChromeModel {
  private(set) var activity = WritingChromeActivityState()
  private(set) var closePrompt: (title: String, message: String)?
  var isConfirmingClose = false

  var opacity: Double { WritingChromeMetrics.standard.opacity(for: activity.visibility) }

  func send(_ event: WritingChromeActivityEvent) {
    var next = activity
    next.reduce(event)
    let duration = WritingChromeMetrics.standard.fadeDuration(into: next.visibility)
    withAnimation(.easeOut(duration: duration)) { activity = next }
  }

  /// Closes the pane at once, or asks first when its file has unsaved changes.
  func requestClose(_ context: WritingPaneContext) {
    switch MacPaneLayoutMetrics.chromePolicy.closeDisposition(
      fileName: context.documentFileName, dirty: context.file?.file.dirty == true)
    {
    case .closeImmediately:
      context.client.closePane(context.pane.paneId)
    case .confirm(let title, let message):
      closePrompt = (title, message)
      isConfirmingClose = true
    }
  }
}

private struct WritingPaneLifecycle: ViewModifier {
  let context: WritingPaneContext
  @Bindable var chrome: WritingChromeModel
  let loadsOnlyWithoutViewport: Bool

  func body(content: Content) -> some View {
    content
      .task(id: context.loadKey) {
        await context.loadIfNeeded(onlyWithoutViewport: loadsOnlyWithoutViewport)
      }
      .alert(chrome.closePrompt?.title ?? "", isPresented: $chrome.isConfirmingClose) {
        Button("Cancel", role: .cancel) {}
        Button("Close", role: .destructive) { context.client.closePane(context.pane.paneId) }
      } message: {
        Text(chrome.closePrompt?.message ?? "")
      }
  }
}

extension View {
  /// Loads the pane's document when it appears or changes, and shows its close question.
  func writingPaneLifecycle(
    _ context: WritingPaneContext, chrome: WritingChromeModel,
    loadsOnlyWithoutViewport: Bool = false
  ) -> some View {
    modifier(
      WritingPaneLifecycle(
        context: context, chrome: chrome, loadsOnlyWithoutViewport: loadsOnlyWithoutViewport))
  }
}

/// A pane whose document and layout are ready, with what its surface needs from them.
struct WritingResolvedPane<Ready> {
  let plan: WritingLayoutPlan
  let ready: Ready
}

/// The frame of every writing pane: the planned page inside a scroll view that only scrolls
/// along the axes the plan overflows, the header at the plan's header rect, and a progress
/// placeholder until `resolve` has a plan.
struct WritingPaneChrome<Ready, Header: View, Surface: View>: View {
  let chrome: WritingChromeModel
  /// Whether the planned page is clipped to the card's corners as well as the scroll view.
  var clipsPage = false
  let resolve: (CGSize) -> WritingResolvedPane<Ready>?
  /// The header; `nil` while the placeholder is showing.
  @ViewBuilder let header: (Ready?) -> Header
  @ViewBuilder let surface: (WritingLayoutPlan, Ready) -> Surface

  var body: some View {
    GeometryReader { proxy in
      if let resolved = resolve(proxy.size) {
        ScrollView(
          Axis.Set(overflow: resolved.plan.overflowAxes),
          showsIndicators: !resolved.plan.overflowAxes.isEmpty
        ) {
          page(resolved.plan, ready: resolved.ready)
            .forceEnclosingOverlayScrollers()
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDisabled(resolved.plan.overflowAxes.isEmpty)
        .frame(width: proxy.size.width, height: proxy.size.height)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
        .paneShell()
      } else {
        VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
          header(nil)
          ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(PaneLayoutContentMetrics.paneCardInset)
        .paneShell()
      }
    }
  }

  private func page(_ plan: WritingLayoutPlan, ready: Ready) -> some View {
    ZStack(alignment: .topLeading) {
      // Give the layout coordinate space its full planned size before positioning children.
      // Positioned children do not contribute their position to the ZStack's ideal size.
      Color.clear
        .frame(width: plan.contentSize.width, height: plan.contentSize.height)
        .allowsHitTesting(false)
      MacTheme.face
        .writingFrame(plan.contentRect)
      header(ready)
        .writingFrame(plan.headerRect)
      surface(plan, ready)
    }
    .frame(
      width: plan.contentSize.width, height: plan.contentSize.height,
      alignment: .topLeading
    )
    .clipped(if: clipsPage)
    .onContinuousHover { phase in
      if case .active = phase { chrome.send(.pointerMoved) }
    }
  }
}

extension View {
  @ViewBuilder fileprivate func clipped(if clips: Bool) -> some View {
    if clips {
      clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    } else {
      self
    }
  }

  /// Takes the view out of the keyboard focus order.
  @ViewBuilder fileprivate func skippingKeyboardFocus(_ skips: Bool) -> some View {
    if skips {
      focusable(false)
    } else {
      self
    }
  }
}

/// The accepted P5/P2 writing header: the file name, its count, and the pane's trailing control.
struct WritingChromeHeader<Trailing: View>: View {
  let paneId: String
  let title: String
  let metadata: String?
  let companionLinked: Bool
  let opacity: Double
  let headerDrag: PaneHeaderDragConfiguration?
  let onHover: (Bool) -> Void
  @ViewBuilder let trailing: () -> Trailing

  var body: some View {
    HStack(spacing: WritingChromeMetrics.standard.headerActionGap) {
      HStack(spacing: MacTheme.Space.x6) {
        Text(title)
          .font(
            MacTheme.chrome(
              WritingChromeMetrics.standard.headerFontPointSize, weight: .semibold)
          )
          .foregroundStyle(MacTheme.secondary)
          .lineLimit(1)
          .help(title)
          .accessibilityLabel(title)
        if companionLinked {
          Image(systemName: "link")
            .font(.system(size: WritingChromeMetrics.standard.headerMenuIconSize))
            .foregroundStyle(MacTheme.secondary)
            .help("Linked writing companion")
            .accessibilityLabel("Linked writing companion")
            .accessibilityIdentifier("writing-companion-link-\(paneId)")
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      if let metadata {
        Text(metadata)
          .font(MacTheme.chrome(WritingChromeMetrics.standard.headerFontPointSize))
          .foregroundStyle(MacTheme.muted)
          .lineLimit(1)
      }
      trailing()
    }
    .padding(.horizontal, WritingChromeMetrics.standard.headerHorizontalInset)
    .frame(height: WritingChromeMetrics.standard.headerHeight)
    .background(MacTheme.face)
    .opacity(opacity)
    .onHover(perform: onHover)
    .paneHeaderDrag(headerDrag)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("writing-header-\(paneId)")
  }
}

/// The ellipsis that opens a writing pane's options.
struct WritingOptionsButton: View {
  let paneId: String
  /// "Writing", "Screenplay" or "Presentation".
  let name: String
  var skipsKeyboardFocus = false
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      Image(systemName: "ellipsis")
        .font(.system(size: WritingChromeMetrics.standard.headerMenuIconSize))
        .foregroundStyle(MacTheme.secondary)
        .frame(
          width: WritingChromeMetrics.standard.headerMenuTargetSize,
          height: WritingChromeMetrics.standard.headerMenuTargetSize
        )
        .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .skippingKeyboardFocus(skipsKeyboardFocus)
    .help("\(name) options")
    .accessibilityLabel("\(name) options menu")
    .accessibilityIdentifier("writing-options-\(paneId)")
  }
}

struct WritingOptionLabel: View {
  let title: String
  let icon: String
  var active = false

  var body: some View {
    Label(title, systemImage: icon)
      .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
      .foregroundStyle(active ? MacTheme.accent : MacTheme.ink)
      .frame(
        maxWidth: .infinity,
        minHeight: PaneLayoutContentMetrics.writingOptionsRowHeight,
        alignment: .leading
      )
      .contentShape(Rectangle())
  }
}

/// One row of a writing pane's options menu.
struct WritingOptionButton: View {
  let title: String
  let icon: String
  var role: ButtonRole?
  var active = false
  var disabled = false
  let identifier: String
  /// True for a row that must not take the keyboard focus from the document; false hides only
  /// the focus ring.
  var skipsKeyboardFocus = true
  let action: () -> Void

  /// A row for `action`, titled and identified as `WritingChromeAction` says.
  init(
    _ action: WritingChromeAction, paneId: String, title: String? = nil, icon: String? = nil,
    role: ButtonRole? = nil, active: Bool = false, disabled: Bool = false,
    skipsKeyboardFocus: Bool = true, perform: @escaping () -> Void
  ) {
    self.title = title ?? action.title()
    self.icon = icon ?? action.systemImage
    self.role = role
    self.active = active
    self.disabled = disabled
    self.identifier = action.accessibilityIdentifier(paneId: paneId)
    self.skipsKeyboardFocus = skipsKeyboardFocus
    self.action = perform
  }

  var body: some View {
    let button = Button(role: role, action: action) {
      WritingOptionLabel(title: title, icon: icon, active: active)
    }
    .buttonStyle(.plain)
    Group {
      if skipsKeyboardFocus {
        button.focusable(false)
      } else {
        button.focusEffectDisabled()
      }
    }
    .disabled(disabled)
    .accessibilityIdentifier(identifier)
  }
}

/// A writing pane's options menu: the rows of its `WritingChromePlan`. The split and close rows
/// are the same everywhere; `specific` draws the rest.
struct WritingOptionsMenu<Specific: View>: View {
  let name: String
  let paneId: String
  let groups: [[WritingChromeAction]]
  let connected: Bool
  let layoutActionsDisabled: Bool
  var skipsKeyboardFocus = true
  let dismiss: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onClose: () -> Void
  @ViewBuilder let specific: (WritingChromeAction) -> Specific

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
        if index > 0 { Divider().padding(.vertical, MacTheme.Space.x3) }
        ForEach(group, id: \.rawValue) { action in
          row(action)
        }
      }
    }
    .padding(MacTheme.Space.x8)
    .frame(width: PaneLayoutContentMetrics.writingOptionsMenuWidth)
    .background(MacTheme.face)
    .accessibilityElement(children: .contain)
    .accessibilityLabel("\(name) options menu")
  }

  @ViewBuilder private func row(_ action: WritingChromeAction) -> some View {
    switch action {
    case .splitColumns, .splitRows:
      WritingOptionButton(
        action, paneId: paneId, disabled: !connected || layoutActionsDisabled,
        skipsKeyboardFocus: skipsKeyboardFocus
      ) {
        dismiss()
        onSplit(paneId, action == .splitColumns ? .columns : .rows)
      }
    case .closePane:
      WritingOptionButton(
        action, paneId: paneId, role: .destructive,
        disabled: !connected || layoutActionsDisabled, skipsKeyboardFocus: skipsKeyboardFocus
      ) {
        dismiss()
        onClose()
      }
    default:
      specific(action)
    }
  }
}

/// A navigation rail: an upper-case title with its count above a scrolling list.
struct WritingRail<Content: View>: View {
  let title: String
  let count: Int
  let titleSize: CGFloat
  var titleTracking: CGFloat = 0
  var width: CGFloat?
  let itemSpacing: CGFloat
  var contentInset: CGFloat = 0
  var forcesOverlayScrollers = false
  @ViewBuilder let content: () -> Content

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      HStack {
        Text(title)
          .font(MacTheme.chrome(titleSize, weight: .bold))
          .tracking(titleTracking).foregroundStyle(MacTheme.secondary)
        Spacer()
        Text("\(count)")
          .font(MacTheme.mono(titleSize)).foregroundStyle(MacTheme.muted)
      }
      ScrollView {
        LazyVStack(spacing: itemSpacing) { content() }
          .padding(contentInset)
          .scrollerStyle(forcesOverlay: forcesOverlayScrollers)
      }
    }
    .padding(MacTheme.Space.x12)
    .frame(width: width)
    .background(MacTheme.face)
  }
}

extension View {
  @ViewBuilder fileprivate func scrollerStyle(forcesOverlay: Bool) -> some View {
    if forcesOverlay {
      forceEnclosingOverlayScrollers()
    } else {
      self
    }
  }
}

/// The hairline between a navigation rail and the page beside it.
struct WritingRailDivider: View {
  let rail: PaneLayoutRect

  var body: some View {
    MacTheme.well.writingFrame(WritingLayoutMetrics.standard.railDivider(after: rail))
  }
}
