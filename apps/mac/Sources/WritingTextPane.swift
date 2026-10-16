import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingTextPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let companionLinked: Bool
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var selection: SoyehtProtocol.TextRange?
  @State private var selectionGeometry: PaneLayoutRect?
  @State private var activeLine: UInt32 = 0
  @State private var destinationKind: WritingFormatKind?
  @State private var destination = ""
  @State private var showingOptions = false
  @State private var clearPointerOpenedOptionsFocus = false
  @State private var chrome = WritingChromeModel()

  private var context: WritingPaneContext { WritingPaneContext(client: client, pane: pane) }

  var body: some View {
    WritingPaneChrome(
      chrome: chrome,
      resolve: { size -> WritingResolvedPane<WritingDocument>? in
        guard let document = context.writing?.document,
          let plan = layoutPlan(size: size, document: document)
        else { return nil }
        return WritingResolvedPane(plan: plan, ready: document)
      },
      header: { writingHeader(document: $0) },
      surface: { plan, document in plannedSurface(plan: plan, document: document) }
    )
    .writingPaneLifecycle(context, chrome: chrome, loadsOnlyWithoutViewport: true)
    .onChange(of: hasSelection) { _, value in
      chrome.send(.selectionChanged(value))
      if !value { selectionGeometry = nil }
    }
    .onChange(of: showingOptions) { _, value in chrome.send(.menuChanged(value)) }
    .onChange(of: destinationKind != nil) { _, value in
      chrome.send(.popoverChanged(value))
    }
  }

  private func layoutPlan(size: CGSize, document: WritingDocument) -> WritingLayoutPlan? {
    MacPaneLayoutMetrics.writingPlan(
      kind: .text, size: size, font: MacPaneLayoutMetrics.proseMetric(for: document.font),
      fullWidth: document.fullWidth, reviewVisible: context.file?.review != nil)
  }

  @ViewBuilder private func plannedSurface(
    plan: WritingLayoutPlan, document: WritingDocument
  ) -> some View {
    let file = context.file
    if let reviewRect = plan.reviewRect, let file {
      WritingTextReviewStrip(
        client: client, view: file, selectedPosition: selection?.start
      )
      .writingFrame(reviewRect)
    }
    if let file, file.viewport != nil,
      case .text(let textPlan) = plan.surface
    {
      WritingTextViewport(
        client: client, view: file, document: document,
        selection: $selection, selectionGeometry: $selectionGeometry,
        activeLine: $activeLine, onEditingActivity: { chrome.send(.edited) },
        onFormatShortcut: { kind, range in apply(kind, range: range) }
      )
      .writingFrame(textPlan.columnRect)
      .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
      .accessibilityIdentifier("writing-text-column-\(pane.paneId)")
    } else if case .text(let textPlan) = plan.surface {
      ProgressView()
        .writingFrame(textPlan.columnRect)
    }
    if let selectionGeometry,
      let toolbarRect = selectionToolbarRect(selectionGeometry, plan: plan)
    {
      WritingSelectionToolbar(
        paneId: pane.paneId,
        actions: MacPaneLayoutMetrics.chromePolicy.plan(kind: .text, hasSelection: true)
          .selectionActions,
        connected: client.connected, hasSelection: hasSelection,
        destinationKind: $destinationKind, destination: $destination,
        apply: { kind, value in apply(kind, value: value) }
      )
      .writingFrame(toolbarRect)
      .zIndex(2)
    }
  }

  private func writingHeader(document: WritingDocument?) -> some View {
    WritingChromeHeader(
      paneId: pane.paneId, title: context.documentFileName,
      metadata: wordCountLabel,
      companionLinked: companionLinked, opacity: chrome.opacity,
      headerDrag: headerDrag,
      onHover: { chrome.send(.headerHoverChanged($0)) },
      trailing: {
        WritingOptionsButton(paneId: pane.paneId, name: "Writing") {
          if !showingOptions {
            clearPointerOpenedOptionsFocus = NSApp.currentEvent?.type == .leftMouseUp
          }
          showingOptions.toggle()
        }
        .popover(isPresented: $showingOptions, arrowEdge: .bottom) {
          if let document {
            writingOptions(document)
              .background(
                MacWindowReader { window in
                  guard clearPointerOpenedOptionsFocus, let window else { return }
                  window.makeFirstResponder(nil)
                  clearPointerOpenedOptionsFocus = false
                })
          }
        }
      }
    )
  }

  private func writingOptions(_ document: WritingDocument) -> some View {
    WritingOptionsMenu(
      name: "Writing", paneId: pane.paneId,
      groups: MacPaneLayoutMetrics.chromePolicy.plan(
        kind: .text, fullWidth: document.fullWidth
      ).menuGroups,
      connected: client.connected, layoutActionsDisabled: layoutActionsDisabled,
      skipsKeyboardFocus: false,
      dismiss: { showingOptions = false }, onSplit: onSplit,
      onClose: { chrome.requestClose(context) },
      specific: { action in
        switch action {
        case .font:
          fontMenu(document)
        case .useFullWidth, .useReadingWidth:
          WritingOptionButton(
            action, paneId: pane.paneId, disabled: !client.connected, skipsKeyboardFocus: false
          ) {
            showingOptions = false
            context.setPreferences(fullWidth: action == .useFullWidth)
          }
        case .focusMode:
          WritingOptionButton(
            action, paneId: pane.paneId, title: action.title(focusMode: document.focusMode),
            active: document.focusMode, disabled: !client.connected, skipsKeyboardFocus: false
          ) {
            showingOptions = false
            context.setPreferences(focusMode: !document.focusMode)
          }
        default:
          EmptyView()
        }
      }
    )
  }

  private func fontMenu(_ document: WritingDocument) -> some View {
    Menu {
      ForEach(WritingDisplayText.fonts, id: \.rawValue) { font in
        Button {
          showingOptions = false
          context.setPreferences(font: font)
        } label: {
          let title = WritingDisplayText.fontName(font)
          if document.font == font { Label(title, systemImage: "checkmark") } else { Text(title) }
        }
      }
    } label: {
      WritingOptionLabel(
        title: WritingDisplayText.fontName(document.font),
        icon: WritingChromeAction.font.systemImage)
    }
    .menuStyle(.borderlessButton)
    .focusEffectDisabled()
    .disabled(!client.connected)
    .accessibilityLabel("Font: \(WritingDisplayText.fontName(document.font))")
    .accessibilityIdentifier(WritingChromeAction.font.accessibilityIdentifier(paneId: pane.paneId))
  }

  private var wordCountLabel: String? {
    guard let count = context.writing?.structure?.wordCount else { return nil }
    return WritingDisplayText.count(Int(count), kind: .word)
  }

  private var hasSelection: Bool {
    guard let selection else { return false }
    return selection.start.line != selection.end.line
      || selection.start.column != selection.end.column
  }

  private func selectionToolbarRect(
    _ localSelection: PaneLayoutRect, plan: WritingLayoutPlan
  ) -> PaneLayoutRect? {
    guard hasSelection, case .text(let textPlan) = plan.surface else { return nil }
    let selectionInSurface = PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: textPlan.columnRect.origin.x + localSelection.origin.x,
        y: textPlan.columnRect.origin.y + localSelection.origin.y),
      size: localSelection.size)
    return MacPaneLayoutMetrics.chromePolicy.selectionToolbarRect(
      selection: selectionInSurface, within: plan.contentRect)
  }

  private func apply(
    _ kind: WritingFormatKind, value: String? = nil,
    range explicitRange: SoyehtProtocol.TextRange? = nil
  ) {
    guard let fileId = context.writing?.document.file.fileId,
      let range = explicitRange ?? selection,
      range.start.line != range.end.line || range.start.column != range.end.column
    else { return }
    client.perform {
      _ = try await client.formatWritingDocument(
        fileId: fileId, range: range, kind: kind, value: value)
    }
  }
}

/// The formats of the selected text, drawn from the selection actions of the chrome plan.
private struct WritingSelectionToolbar: View {
  let paneId: String
  let actions: [WritingChromeAction]
  let connected: Bool
  let hasSelection: Bool
  @Binding var destinationKind: WritingFormatKind?
  @Binding var destination: String
  let apply: (WritingFormatKind, String?) -> Void

  var body: some View {
    HStack(spacing: WritingChromeMetrics.standard.toolbarGap) {
      ForEach(actions, id: \.rawValue) { action in
        control(action)
      }
    }
    .padding(.horizontal, WritingChromeMetrics.standard.toolbarHorizontalInset)
    .frame(
      width: WritingChromeMetrics.standard.toolbarSize.width,
      height: WritingChromeMetrics.standard.toolbarSize.height
    )
    .background(MacTheme.face, in: Capsule())
    .shadow(
      color: MacTheme.shadowDark.opacity(0.40),
      radius: WritingChromeMetrics.standard.toolbarShadowBlur / 2,
      x: 0, y: WritingChromeMetrics.standard.toolbarShadowOffset
    )
    .popover(
      isPresented: Binding(
        get: { destinationKind != nil },
        set: { if !$0 { destinationKind = nil } }),
      arrowEdge: .bottom
    ) {
      destinationPopover
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel("Formatting selection")
    .accessibilityIdentifier("writing-selection-toolbar-\(paneId)")
    .disabled(!connected)
  }

  @ViewBuilder private func control(_ action: WritingChromeAction) -> some View {
    if let kind = action.directFormat {
      button(action) { apply(kind, nil) }
    } else if let kind = action.destinationFormat {
      button(action) {
        guard hasSelection else { return }
        destination = ""
        destinationKind = kind
      }
    } else {
      Menu {
        ForEach(action.formatChoices, id: \.kind.rawValue) { choice in
          Button(choice.title) { apply(choice.kind, nil) }
        }
      } label: {
        icon(action)
      }
      .menuStyle(.borderlessButton)
      .menuIndicator(.hidden)
      .frame(
        width: WritingChromeMetrics.standard.toolbarButtonWidth,
        height: WritingChromeMetrics.standard.toolbarSize.height
      )
      .help(action.title())
      .accessibilityLabel(action.title())
      .accessibilityIdentifier(action.accessibilityIdentifier(paneId: paneId))
    }
  }

  private func button(_ action: WritingChromeAction, perform: @escaping () -> Void) -> some View {
    Button(action: perform) {
      icon(action)
    }
    .buttonStyle(.plain)
    .help(action.title())
    .accessibilityLabel(action.title())
    .accessibilityIdentifier(action.accessibilityIdentifier(paneId: paneId))
    .commandShortcut(action.shortcutKey)
  }

  private func icon(_ action: WritingChromeAction) -> some View {
    Image(systemName: action.systemImage)
      .font(
        .system(
          size: WritingChromeMetrics.standard.toolbarIconSize,
          weight: .semibold)
      )
      .foregroundStyle(MacTheme.ink)
      .frame(
        width: WritingChromeMetrics.standard.toolbarButtonWidth,
        height: WritingChromeMetrics.standard.toolbarSize.height
      )
      .contentShape(Rectangle())
      .help(action.title())
  }

  private var destinationPopover: some View {
    let prompt = destinationKind?.destinationPrompt
    return VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      Text(prompt?.title ?? "")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
      TextField(prompt?.placeholder ?? "", text: $destination)
        .textFieldStyle(.plain).font(MacTheme.mono(MacTheme.TypeScale.control))
        .padding(MacTheme.Space.x8).neoWell(radius: MacTheme.Radius.compact)
      HStack {
        Button("Cancel") { destinationKind = nil }
        Spacer()
        Button("Apply") {
          if let kind = destinationKind { apply(kind, destination) }
          destinationKind = nil
        }
        .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(destination.isEmpty)
      }
    }
    .padding(MacTheme.Space.x16).frame(width: 320).background(MacTheme.face)
  }
}

extension View {
  /// Binds Command and `key` to the view's action when there is a key.
  @ViewBuilder fileprivate func commandShortcut(_ key: Character?) -> some View {
    if let key {
      keyboardShortcut(KeyEquivalent(key), modifiers: .command)
    } else {
      self
    }
  }
}

private struct WritingTextReviewStrip: View {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let selectedPosition: TextPosition?
  @State private var showingReview = false

  var body: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "person.crop.circle.badge.checkmark")
        .foregroundStyle(MacTheme.accent)
      Text("Agent changes ready to review")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        .lineLimit(1)
        .frame(maxWidth: .infinity, alignment: .leading)
      Button("Review") { showingReview = true }
        .buttonStyle(NeoPillButtonStyle())
        .accessibilityIdentifier("writing-review-\(view.file.fileId)")
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: PaneLayoutContentMetrics.textReviewHeight)
    .background(MacTheme.canvas)
    .popover(isPresented: $showingReview, arrowEdge: .bottom) {
      EditorAgentReviewBar(
        client: client, view: view, selectedPosition: selectedPosition
      )
      .padding(MacTheme.Space.x10)
      .frame(width: 680)
      .background(MacTheme.canvas)
    }
  }
}
