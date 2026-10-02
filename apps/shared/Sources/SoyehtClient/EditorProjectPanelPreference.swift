/// Whether the editor shows its project panel beside the code, chosen per workspace with the
/// header's sidebar button or Cmd+Shift+E.
public enum EditorProjectPanelPreference: String, Sendable, Equatable {
  /// Shown when the panes fit with it; a narrow layout folds it away (`PaneLayoutFit`).
  case automatic
  /// Kept open: the layout first stacks or tabs the panes, and folds the panel only when not
  /// even that fits the window, so nothing is clipped.
  case shown
  /// Folded away at every width.
  case hidden

  /// The panel is on screen when the person allows it and the pane is wide enough for it.
  public func isShown(fits: Bool) -> Bool {
    self != .hidden && fits
  }

  /// The choice a toggle makes from what is on screen: hide a visible panel, else keep it open.
  public static func toggled(from isShown: Bool) -> Self {
    isShown ? .hidden : .shown
  }
}
