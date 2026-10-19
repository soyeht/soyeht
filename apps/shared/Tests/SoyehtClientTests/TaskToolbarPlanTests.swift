import Testing

@testable import SoyehtClient

@Test func theTaskToolbarGivesUpLabelsThenItsChips() {
  let densities = TaskToolbarDensity.allCases
  #expect(densities == [.full, .icons, .minimal])

  let full = TaskToolbarDensity.full.items(listMode: false)
  #expect(full == [.viewToggle, .filter, .priority, .dispatch, .newTask])
  #expect(TaskToolbarDensity.full.items(listMode: true).contains(.group))
  #expect(TaskToolbarDensity.icons.items(listMode: false) == full)
  #expect(TaskToolbarDensity.minimal.items(listMode: true) == [.viewToggle, .newTask])

  #expect(full.allSatisfy { TaskToolbarDensity.full.showsLabel(of: $0) })
  #expect(TaskToolbarDensity.icons.showsLabel(of: .newTask))
  #expect(!TaskToolbarDensity.icons.showsLabel(of: .filter))
  #expect(!TaskToolbarDensity.minimal.showsLabel(of: .viewToggle))
}

/// "New task" is the one action: every density keeps it, and none drops the view toggle.
@Test func everyTaskToolbarDensityKeepsTheViewToggleAndNewTask() {
  for density in TaskToolbarDensity.allCases {
    for listMode in [false, true] {
      let items = density.items(listMode: listMode)
      #expect(items.first == .viewToggle && items.last == .newTask)
    }
  }
}
