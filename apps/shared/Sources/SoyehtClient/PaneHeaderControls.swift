import SoyehtProtocol

/// An icon action in a pane header. The title is the tooltip, the accessibility label and the
/// label in the compact actions menu, so every header icon is named the same way everywhere.
public enum PaneHeaderAction: String, CaseIterable, Sendable {
  case loop
  case tasks
  case splitColumns
  case splitRows
  case close

  public var title: String {
    switch self {
    case .loop: "Loop and lead controls"
    case .tasks: "Open workspace tasks"
    case .splitColumns: "Split pane into columns"
    case .splitRows: "Split pane into rows"
    case .close: "Close this pane"
    }
  }

  /// Loop delivers a prompt on a schedule, so its icon is a timer: arrows would read as
  /// "switch", which is the harness control's job.
  public var systemImage: String {
    switch self {
    case .loop: "timer"
    case .tasks: "line.3.horizontal.decrease"
    case .splitColumns: "rectangle.split.2x1"
    case .splitRows: "rectangle.split.1x2"
    case .close: "xmark"
    }
  }

  public func accessibilityIdentifier(paneId: String) -> String {
    switch self {
    case .loop: "pane-loop-\(paneId)"
    case .tasks: "pane-tasks-\(paneId)"
    case .splitColumns: "split-pane-columns-\(paneId)"
    case .splitRows: "split-pane-rows-\(paneId)"
    case .close: "close-pane-\(paneId)"
    }
  }
}

/// The icon actions a pane header exposes, in order. An agent pane's harness switch lives in
/// its composer ("Claude ▾"), not in the header.
public struct PaneHeaderControls: Equatable, Sendable {
  public let actions: [PaneHeaderAction]

  public init(mode: PaneMode) {
    actions = (mode == .agent ? [.loop] : []) + [.tasks, .splitColumns, .splitRows, .close]
  }
}
