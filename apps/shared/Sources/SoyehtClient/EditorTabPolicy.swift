import SoyehtProtocol

/// Tab rules shared by every editor surface. Tabs are pane identifiers in the
/// engine's order; `history` lists the panes this client activated, newest first.
public enum EditorTabPolicy {
  /// A preview stays a preview while its file keeps the revision it was
  /// previewed at; the first edit makes it a permanent tab.
  public static func isPreview(_ tab: EditorTab, file: FileDocument?) -> Bool {
    guard tab.preview, let revision = tab.previewRevision else { return false }
    return file.map { $0.revision == revision } ?? true
  }

  /// The tab that shows: the selected pane when it is a tab, else the most
  /// recently activated tab, else the first.
  public static func active(selected: String?, tabs: [String], history: [String]) -> String? {
    if let selected, tabs.contains(selected) { return selected }
    return history.first { tabs.contains($0) } ?? tabs.first
  }

  /// Which tab takes over when `closing` closes, as Zed's default
  /// `activate_on_close: "history"`: the previously active tab, else the right
  /// neighbour, else the left one.
  public static func successor(closing: String, tabs: [String], history: [String]) -> String? {
    let remaining = tabs.filter { $0 != closing }
    if let recent = history.first(where: { $0 != closing && remaining.contains($0) }) {
      return recent
    }
    guard let index = tabs.firstIndex(of: closing) else { return remaining.first }
    if index + 1 < tabs.count { return tabs[index + 1] }
    return index > 0 ? tabs[index - 1] : nil
  }

  /// The next or previous tab, wrapping around (`Cmd+Shift+]` / `Cmd+Shift+[`).
  public static func cycled(from current: String?, tabs: [String], forward: Bool) -> String? {
    guard !tabs.isEmpty else { return nil }
    guard let current, let index = tabs.firstIndex(of: current) else {
      return forward ? tabs.first : tabs.last
    }
    let step = forward ? 1 : tabs.count - 1
    return tabs[(index + step) % tabs.count]
  }

  /// The engine index for dropping `moving` onto the slot of `target`.
  public static func dropIndex(moving: String, onto target: String, tabs: [String]) -> Int? {
    guard tabs.contains(moving), let index = tabs.firstIndex(of: target) else { return nil }
    return index
  }

  /// The order after a move, for showing a drop before the engine confirms it.
  public static func moved(_ tabs: [String], paneId: String, to index: Int) -> [String] {
    guard let from = tabs.firstIndex(of: paneId) else { return tabs }
    var order = tabs
    order.remove(at: from)
    order.insert(paneId, at: min(max(0, index), order.count))
    return order
  }

  /// Puts `paneId` first in the activation history, keeping it bounded.
  public static func recorded(_ paneId: String, in history: [String]) -> [String] {
    Array(([paneId] + history.filter { $0 != paneId }).prefix(64))
  }

  /// Closing a tab saves its draft; only a draft that conflicts with the disk
  /// needs the person's word, and it stays in the engine either way.
  public static func closeNeedsConfirmation(_ file: FileDocument?) -> Bool {
    file?.conflicted == true
  }
}
