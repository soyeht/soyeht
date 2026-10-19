/// The controls of the task board's toolbar, in the order they appear.
public enum TaskToolbarItem: Equatable, Sendable {
  case viewToggle, filter, priority, group, dispatch, newTask
}

/// What the task toolbar keeps as its width runs out. A host tries the densities in order and
/// shows the first that fits (`ViewThatFits`), so no label ever wraps or overlaps its neighbour:
/// 1. every control with its label;
/// 2. the same controls as icons, the view toggle too;
/// 3. the view toggle and "New task" alone: the filter, priority and group chips are only
///    sorting shortcuts and the dispatch menu is also reachable from a task.
public enum TaskToolbarDensity: CaseIterable, Sendable {
  case full, icons, minimal

  /// The controls this density draws; `group` shows only in the list view.
  public func items(listMode: Bool) -> [TaskToolbarItem] {
    switch self {
    case .full, .icons:
      [.viewToggle, .filter, .priority] + (listMode ? [.group] : []) + [.dispatch, .newTask]
    case .minimal:
      [.viewToggle, .newTask]
    }
  }

  /// Whether a control carries its text beside its icon; "New task" keeps its label at every
  /// density because it is the one action of the toolbar.
  public func showsLabel(of item: TaskToolbarItem) -> Bool {
    switch self {
    case .full: true
    case .icons, .minimal: item == .newTask
    }
  }
}
