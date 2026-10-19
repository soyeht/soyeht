import Foundation
import Observation

/// The columns and rows of a terminal.
public struct TerminalGrid: Equatable, Sendable {
  public var cols: Int
  public var rows: Int

  public init(cols: Int, rows: Int) {
    self.cols = cols
    self.rows = rows
  }

  public var isUsable: Bool { cols > 0 && rows > 0 }
}

/// Who decides the size of a terminal pane that more than one client shows.
///
/// The engine keeps one size per pane and the last `pane.resize` wins (ADR 0013). A paired
/// device is a remote for the Mac, so by default it follows the size the pane already has and
/// never reflows the Mac. "Fit to this screen" is the person's explicit choice to make the pane
/// match this device instead.
public enum TerminalSizeMode: Equatable, Sendable {
  case followMac
  case fitThisScreen
}

public enum TerminalSizeAction: Equatable, Sendable {
  /// Nothing to do.
  case none
  /// Ask the engine to resize the pane to this grid.
  case request(TerminalGrid)
  /// Show the terminal at the pane's grid; send nothing.
  case adopt(TerminalGrid)
}

public enum TerminalSizePolicy {
  /// The view was shown or its state changed. `announced` says that this pane has already been
  /// sized from here for this connection and mode. `requestInFlight` says that a size asked of
  /// the engine has not come back yet, so the pane's current size is about to change.
  public static func reconcile(
    mode: TerminalSizeMode, pane: TerminalGrid, local: TerminalGrid, announced: Bool,
    requestInFlight: Bool
  ) -> TerminalSizeAction {
    switch mode {
    case .followMac:
      return adoptIfDifferent(pane, local: local)
    case .fitThisScreen:
      if !announced { return local.isUsable ? .request(local) : .none }
      return requestInFlight ? .none : adoptIfDifferent(pane, local: local)
    }
  }

  /// The view's own layout produced a new grid (rotation, split resize, text size).
  public static func layoutChanged(
    mode: TerminalSizeMode, pane: TerminalGrid?, natural: TerminalGrid
  ) -> TerminalSizeAction {
    switch mode {
    case .fitThisScreen:
      return natural.isUsable ? .request(natural) : .none
    case .followMac:
      guard let pane, pane.isUsable else { return .none }
      return .adopt(pane)
    }
  }

  private static func adoptIfDifferent(_ pane: TerminalGrid, local: TerminalGrid)
    -> TerminalSizeAction
  {
    pane.isUsable && pane != local ? .adopt(pane) : .none
  }
}

/// The size mode of each pane on this device. It lasts as long as the app runs: a pane always
/// opens following the Mac. Choosing "Fit to this screen" remembers the size the pane had, and
/// going back to the Mac's size puts it back.
@MainActor @Observable
public final class TerminalSizeModeStore {
  public static let shared = TerminalSizeModeStore()

  private var modes: [String: TerminalSizeMode] = [:]
  @ObservationIgnored private var sizeBeforeFit: [String: TerminalGrid] = [:]

  public init() {}

  public func mode(for paneId: String) -> TerminalSizeMode { modes[paneId] ?? .followMac }

  /// Switches `paneId` to `mode`. Returns the size to ask the engine for, when leaving a fit
  /// for the size the pane had before it.
  func choose(_ mode: TerminalSizeMode, for paneId: String, paneGrid: TerminalGrid?)
    -> TerminalGrid?
  {
    switch mode {
    case .fitThisScreen:
      if modes[paneId] == nil, let paneGrid, paneGrid.isUsable {
        sizeBeforeFit[paneId] = paneGrid
      }
      modes[paneId] = .fitThisScreen
      return nil
    case .followMac:
      let wasFitted = modes.removeValue(forKey: paneId) != nil
      let previous = sizeBeforeFit.removeValue(forKey: paneId)
      guard wasFitted, let previous, previous != paneGrid else { return nil }
      return previous
    }
  }

  public func choose(_ mode: TerminalSizeMode, for paneId: String, client: EngineClient) {
    let pane = client.panes.first { $0.paneId == paneId }
    let grid = pane.map { TerminalGrid(cols: Int($0.cols), rows: Int($0.rows)) }
    if let restore = choose(mode, for: paneId, paneGrid: grid) {
      client.resize(paneId: paneId, cols: restore.cols, rows: restore.rows)
    }
  }
}
