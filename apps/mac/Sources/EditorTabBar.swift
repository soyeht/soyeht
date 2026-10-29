import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The editor's tab strip. Order, preview state and activation rules live in
/// the engine and `EditorTabPolicy`; this view only draws them and forwards
/// clicks, drags and the Zed-style shortcuts.
struct EditorTabBar: View {
  @Bindable var client: EngineClient
  let selected: SoyehtProtocol.FileDocument
  @State private var confirmClose: EditorTab?

  private var workspaceId: String { selected.workspaceId }
  private var tabs: [EditorTab] { client.editorTabs(in: workspaceId) }

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      ScrollView(.horizontal) {
        HStack(spacing: MacTheme.Space.x4) {
          ForEach(tabs, id: \.paneId) { tab in
            EditorTabItem(
              tab: tab, file: client.editorFiles[tab.fileId]?.file,
              active: tab.paneId == selected.paneId,
              activate: { client.activateEditorTab(tab.paneId) },
              pin: { pin(tab) }, close: { requestClose(tab) },
              drop: { moving in move(moving, onto: tab) })
          }
        }
      }
      .scrollIndicators(.hidden)
      if let cursor = agentCursor {
        HStack(spacing: MacTheme.Space.x6) {
          Circle().fill(MacTheme.ink).frame(width: MacTheme.Space.x6, height: MacTheme.Space.x6)
          Text(
            "\(cursor.name) · \(UInt64(cursor.position.line) + 1):\(UInt64(cursor.position.column) + 1)"
          )
          .font(MacTheme.mono(EditorTabMetrics.agentFont, weight: .bold))
          .foregroundStyle(MacTheme.ink)
        }
        .padding(.horizontal, MacTheme.Space.x10).frame(height: EditorTabMetrics.chipHeight)
        .background(MacTheme.agentBlue, in: Capsule())
      }
    }
    .padding(.horizontal, MacTheme.Space.x8).frame(height: EditorTabMetrics.barHeight)
    .background(MacTheme.canvas)
    .background(
      EditorTabShortcutMonitor { shortcut in handle(shortcut) }
    )
    .task(id: "\(workspaceId):\(client.connectionRevision)") {
      await client.loadEditorTabsIfNeeded(workspaceId: workspaceId)
    }
    .alert(
      "Close \(confirmClose.map { fileName($0.path) } ?? "tab")?",
      isPresented: Binding(get: { confirmClose != nil }, set: { if !$0 { confirmClose = nil } })
    ) {
      Button("Keep editing", role: .cancel) { confirmClose = nil }
      Button("Close tab") {
        if let tab = confirmClose { close(tab) }
        confirmClose = nil
      }
    } message: {
      Text(
        "The file changed on disk while it had unsaved edits. The draft stays with Soyeht and comes back, with the conflict, when you open the file again."
      )
    }
  }

  private var agentCursor: EditorAgentCursorEvent? {
    client.editorFiles[selected.fileId]?.agentCursors.values.sorted { $0.name < $1.name }.first
  }

  /// Shortcuts act only for the editor pane the person is working in.
  private func handle(_ shortcut: EditorTabShortcut) -> Bool {
    guard client.selectedPaneId == selected.paneId else { return false }
    switch shortcut {
    case .close:
      if let tab = tabs.first(where: { $0.paneId == selected.paneId }) { requestClose(tab) }
    case .next: client.cycleEditorTab(in: workspaceId, forward: true)
    case .previous: client.cycleEditorTab(in: workspaceId, forward: false)
    }
    return true
  }

  private func requestClose(_ tab: EditorTab) {
    if EditorTabPolicy.closeNeedsConfirmation(client.editorFiles[tab.fileId]?.file) {
      confirmClose = tab
    } else {
      close(tab)
    }
  }

  private func close(_ tab: EditorTab) {
    Task { @MainActor in await client.closePaneSavingEdits(tab.paneId) }
  }

  private func pin(_ tab: EditorTab) {
    Task { @MainActor in
      do { try await client.openEditorFile(workspaceId: workspaceId, path: tab.path) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func move(_ moving: String, onto target: EditorTab) -> Bool {
    guard moving != target.paneId,
      let index = EditorTabPolicy.dropIndex(
        moving: moving, onto: target.paneId, tabs: tabs.map(\.paneId))
    else { return false }
    Task { @MainActor in
      do {
        try await client.moveEditorTab(workspaceId: workspaceId, paneId: moving, to: index)
      } catch { client.problem = error.localizedDescription }
    }
    return true
  }

  private func fileName(_ path: String) -> String {
    String(path.split(separator: "/").last ?? "File")
  }
}

private enum EditorTabMetrics {
  static let barHeight: CGFloat = 34
  static let chipHeight: CGFloat = 22
  static let tabFont: CGFloat = 11.5
  static let agentFont: CGFloat = 10.5
  static let closeSlot: CGFloat = 12
  static let dirtyDot: CGFloat = 6
}

private struct EditorTabItem: View {
  let tab: EditorTab
  let file: SoyehtProtocol.FileDocument?
  let active: Bool
  let activate: () -> Void
  let pin: () -> Void
  let close: () -> Void
  let drop: (String) -> Bool
  @State private var hovering = false

  private var preview: Bool { EditorTabPolicy.isPreview(tab, file: file) }

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Image(systemName: "doc.text")
        .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .medium))
      Text(String(tab.path.split(separator: "/").last ?? "File"))
        .font(MacTheme.mono(EditorTabMetrics.tabFont, weight: active ? .bold : .regular))
        .italic(preview)
        .lineLimit(1)
      closeSlot
    }
    .foregroundStyle(active ? MacTheme.accent : MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.editorTabHeight)
    .background(active ? MacTheme.well : hovering ? MacTheme.hover : Color.clear, in: Capsule())
    .contentShape(Capsule())
    .onHover { hovering = $0 }
    .onTapGesture(count: 2, perform: pin)
    .onTapGesture(perform: activate)
    .overlay(EditorMiddleClickArea(action: close))
    .draggable(tab.paneId)
    .dropDestination(for: String.self) { items, _ in
      items.first.map(drop) ?? false
    }
    .help(preview ? "\(tab.path) · preview" : tab.path)
    .accessibilityElement(children: .combine)
    .accessibilityAddTraits(active ? .isSelected : [])
    .accessibilityAction(named: "Close tab", close)
  }

  /// Zed's `show_close_button: "hover"`: the × appears on hover; otherwise an
  /// unsaved file shows its dot.
  @ViewBuilder private var closeSlot: some View {
    ZStack {
      if hovering {
        Button(action: close) {
          Image(systemName: "xmark")
            .font(MacTheme.ui(MacTheme.TypeScale.micro, weight: .semibold))
        }
        .buttonStyle(.plain)
        .help("Close tab (⌘W)")
      } else if file?.dirty == true {
        Circle().fill(MacTheme.warning)
          .frame(width: EditorTabMetrics.dirtyDot, height: EditorTabMetrics.dirtyDot)
      }
    }
    .frame(width: EditorTabMetrics.closeSlot)
  }
}

/// Catches a middle click without taking left clicks or drags from SwiftUI.
struct EditorMiddleClickArea: NSViewRepresentable {
  let action: () -> Void

  func makeNSView(context: Context) -> MiddleClickView {
    let view = MiddleClickView()
    view.action = action
    return view
  }

  func updateNSView(_ nsView: MiddleClickView, context: Context) {
    nsView.action = action
  }

  final class MiddleClickView: NSView {
    var action: (() -> Void)?

    override func hitTest(_ point: NSPoint) -> NSView? {
      let type = NSApp.currentEvent?.type
      guard type == .otherMouseDown || type == .otherMouseUp else { return nil }
      return super.hitTest(point)
    }

    override func otherMouseUp(with event: NSEvent) {
      guard event.buttonNumber == 2 else { return super.otherMouseUp(with: event) }
      action?()
    }
  }
}

enum EditorTabShortcut {
  case close, next, previous
}

/// `Cmd+W`, `Cmd+Shift+]` and `Cmd+Shift+[` while an editor pane is in use.
/// The handler returns false to let the key reach the menu as before.
struct EditorTabShortcutMonitor: NSViewRepresentable {
  let handle: @MainActor (EditorTabShortcut) -> Bool

  func makeCoordinator() -> Coordinator { Coordinator() }

  func makeNSView(context: Context) -> NSView {
    context.coordinator.handle = handle
    context.coordinator.start()
    return NSView(frame: .zero)
  }

  func updateNSView(_ nsView: NSView, context: Context) {
    context.coordinator.handle = handle
  }

  static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
    coordinator.stop()
  }

  @MainActor final class Coordinator {
    var handle: (@MainActor (EditorTabShortcut) -> Bool)?
    private var monitor: Any?

    func start() {
      guard monitor == nil else { return }
      monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
        guard let self, let shortcut = Self.shortcut(event), self.handle?(shortcut) == true
        else { return event }
        return nil
      }
    }

    func stop() {
      if let monitor { NSEvent.removeMonitor(monitor) }
      monitor = nil
    }

    private static func shortcut(_ event: NSEvent) -> EditorTabShortcut? {
      // Typing never carries Command, so ordinary keys leave after one flag test.
      guard event.modifierFlags.contains(.command) else { return nil }
      let flags = event.modifierFlags.intersection([.command, .shift, .option, .control])
      switch (flags, event.charactersIgnoringModifiers) {
      case ([.command], "w"): return .close
      case ([.command, .shift], "}"), ([.command, .shift], "]"): return .next
      case ([.command, .shift], "{"), ([.command, .shift], "["): return .previous
      default: return nil
      }
    }
  }
}
