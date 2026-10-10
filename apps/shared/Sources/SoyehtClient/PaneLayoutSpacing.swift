/// Spacing of the workspace pane grid, measured in the approved desktop frames of
/// design/soyeht-desktop.pen. The window content sits 18 pt inside the screen edges; the
/// "Grade de panes" adds 6 pt of padding, so two pane cards sit 18 pt apart and the outermost
/// cards sit 24 pt inside the content edges, also 24 pt below the top bar (Telas 1 to 13). With
/// the app rail open (Tela 12) the 56 pt rail starts at the 18 pt screen padding and the grid
/// follows it, so the first card sits 6 pt after the rail: 80 pt from the window's left edge,
/// while the right edge keeps 24 pt. The design is asymmetric on purpose there; the rail's
/// buttons end 16 pt before the first card.
///
/// A pane footprint is its card: the card's shell shadow (`PaneShellMetrics.halo`, 15 pt) is
/// drawn outside the footprint, into the margins and into the gap it shares with its
/// neighbour, exactly as in the approved frames. Nothing around a footprint may clip it. The
/// gap is also the divider's hit area, so dragging a divider still grabs the whole space
/// between two cards.
public enum PaneLayoutSpacing {
  /// The visible space between two neighbouring pane cards.
  public static let paneGap = Double(DesignTokens.PaneLayout.paneGap)
  /// The window content's padding inside the screen edges, where the open rail starts.
  public static let screenPadding = Double(DesignTokens.PaneLayout.screenPadding)
  /// The grid's padding around its cards, also the space between the open rail and a card.
  public static let gridPadding = Double(DesignTokens.PaneLayout.gridPadding)
  /// The visible space between the outermost pane cards and the window's content edges.
  public static let outerMargin = screenPadding + gridPadding
  /// The pane footprint keeps no halo of its own; see the type's documentation.
  public static let paneHalo = 0.0
  public static let gaps = PaneLayoutGaps(horizontal: paneGap, vertical: paneGap)
}

/// Where the open rail and the pane grid sit across a window, measured from its left edge.
public struct PaneLayoutHorizontalPlacement: Equatable, Sendable {
  /// The rail's leading edge.
  public var railLeading: Double
  /// The first pane card's leading edge.
  public var paneLeading: Double
  /// The space between the last pane card and the window's right edge.
  public var paneTrailing: Double
  /// A window too narrow for the rail beside the panes draws it over them.
  public var railOverlays: Bool
}

extension PaneLayoutWindowPolicy {
  /// The rail starts in the grid's padding, so beside the panes the first card sits
  /// `gridPadding` after it, as in Tela 12. A narrow window shrinks the inset first.
  public func horizontalPlacement(
    windowWidth: Double, railOpen: Bool, gridPadding: Double = PaneLayoutSpacing.gridPadding
  ) -> PaneLayoutHorizontalPlacement {
    let inset = horizontalInset(windowWidth: windowWidth)
    let overlays = railOverlays(windowWidth: windowWidth)
    return PaneLayoutHorizontalPlacement(
      railLeading: max(0, inset - gridPadding),
      paneLeading: inset + (railOpen && !overlays ? railWidth : 0), paneTrailing: inset,
      railOverlays: overlays)
  }
}
