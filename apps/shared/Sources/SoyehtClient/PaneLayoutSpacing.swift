/// Spacing of the workspace pane grid, measured in the approved desktop frames of
/// design/soyeht-desktop.pen (the "Grade de panes" of Telas 1 to 13): two pane cards sit 18 pt
/// apart, and the outermost cards sit 24 pt inside the content edges (18 pt screen padding plus
/// 6 pt grid padding), also 24 pt below the top bar. With the app rail open (Tela 12) the grid
/// starts right after the 56 pt rail, so the first card sits 6 pt after it.
///
/// A pane footprint is its card: the card's shell shadow (`PaneShellMetrics.halo`, 15 pt) is
/// drawn outside the footprint, into the margins and into the gap it shares with its
/// neighbour, exactly as in the approved frames. Nothing around a footprint may clip it. The
/// gap is also the divider's hit area, so dragging a divider still grabs the whole space
/// between two cards.
public enum PaneLayoutSpacing {
  /// The visible space between two neighbouring pane cards.
  public static let paneGap = 18.0
  /// The visible space between the outermost pane cards and the window's content edges.
  public static let outerMargin = 24.0
  /// The space between the open app rail and the first pane card (the grid's padding).
  public static let railGap = 6.0
  /// The pane footprint keeps no halo of its own; see the type's documentation.
  public static let paneHalo = 0.0
  public static let gaps = PaneLayoutGaps(horizontal: paneGap, vertical: paneGap)
}
