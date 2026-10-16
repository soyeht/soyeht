import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let chromePolicy = WritingChromePolicy()

@Test func textChromeMovesEveryFixedActionWithoutLosingTheSelectionTools() {
  let reading = chromePolicy.plan(kind: .text, hasSelection: true)
  #expect(
    reading.menuGroups == [
      [.font, .useFullWidth, .focusMode], [.splitColumns, .splitRows], [.closePane],
    ])
  #expect(
    reading.selectionActions == [.bold, .italic, .heading, .list, .quote, .link, .image])

  let full = chromePolicy.plan(kind: .text, fullWidth: true, hasSelection: false)
  #expect(full.menuActions[1] == .useReadingWidth)
  #expect(full.selectionActions.isEmpty)
}

@Test func screenplayAndPresentationMenusKeepTheirOnlyDoors() {
  #expect(
    chromePolicy.plan(kind: .screenplay).menuGroups == [
      [.ideaBoard, .scenes, .focusMode], [.splitColumns, .splitRows], [.closePane],
    ])
  #expect(
    chromePolicy.plan(kind: .presentation).menuGroups
      == [[.present], [.splitColumns, .splitRows], [.closePane]])
}

@Test func screenplayElementMenuOnlyAppearsForAnEmptyInsertionLine() {
  #expect(chromePolicy.showsScreenplayElementMenu(lineText: "", selectionLength: 0))
  #expect(chromePolicy.showsScreenplayElementMenu(lineText: "   \t", selectionLength: 0))
  #expect(!chromePolicy.showsScreenplayElementMenu(lineText: "INT. SHOP", selectionLength: 0))
  #expect(!chromePolicy.showsScreenplayElementMenu(lineText: "", selectionLength: 1))
}

@Test func acceptedP2MetricsHaveOneNamedSource() {
  let metrics = WritingChromeMetrics.standard
  #expect(metrics.headerHeight == 40)
  #expect(metrics.headerHorizontalInset == 18)
  #expect(metrics.headerFontPointSize == 14)
  #expect(metrics.headerActionGap == 14)
  #expect(metrics.headerMenuIconSize == 16)
  #expect(metrics.headerMenuTargetSize == 32)
  #expect(metrics.toolbarSize == PaneLayoutSize(width: 232, height: 32))
  #expect(metrics.toolbarButtonWidth == 28)
  #expect(metrics.toolbarIconSize == 16)
  #expect(metrics.toolbarGap == 4)
  #expect(metrics.toolbarHorizontalInset == 6)
  #expect(metrics.selectionGap == 8)
  #expect(metrics.toolbarShadowOffset == 4)
  #expect(metrics.toolbarShadowBlur == 12)
  #expect(metrics.toolbarHalo == 16)
  #expect(metrics.dimmedOpacity == 0.35)
  #expect(PaneLayoutContentMetrics.screenplayInlineElementMenuHeight == 30)
  #expect(PaneLayoutContentMetrics.screenplayInlineElementMenuInset == 12)
  #expect(PaneLayoutContentMetrics.screenplayInlineControlGap == 6)
  #expect(PaneLayoutContentMetrics.screenplayInlineAnchorClearance == 3)
  #expect(PaneLayoutContentMetrics.screenplaySmartTypeFontPointSize == 11)
}

@Test func toolbarPrefersAboveThenBelowAndClampsItsHalo() {
  let available = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 0, y: 40), size: PaneLayoutSize(width: 375, height: 500))
  let middle = chromePolicy.selectionToolbarRect(
    selection: PaneLayoutRect(
      origin: PaneLayoutPoint(x: 100, y: 180), size: PaneLayoutSize(width: 80, height: 26)),
    within: available)
  #expect(middle.origin.y == 140)
  #expect(middle.size == PaneLayoutSize(width: 232, height: 32))

  let top = chromePolicy.selectionToolbarRect(
    selection: PaneLayoutRect(
      origin: PaneLayoutPoint(x: 0, y: 48), size: PaneLayoutSize(width: 24, height: 26)),
    within: available)
  #expect(top.origin.x == 16)
  #expect(top.origin.y == 82)

  let bottom = chromePolicy.selectionToolbarRect(
    selection: PaneLayoutRect(
      origin: PaneLayoutPoint(x: 360, y: 520), size: PaneLayoutSize(width: 15, height: 20)),
    within: available)
  #expect(bottom.origin.x == 127)
  #expect(bottom.origin.y == 480)
}

@Test func typingDimsButEveryInteractiveChromeStateForcesVisibility() {
  var state = WritingChromeActivityState()
  state.reduce(.edited)
  #expect(state.visibility == .dimmed)
  state.reduce(.selectionChanged(true))
  #expect(state.visibility == .visible)
  state.reduce(.selectionChanged(false))
  #expect(state.visibility == .dimmed)
  state.reduce(.menuChanged(true))
  #expect(state.visibility == .visible)
  state.reduce(.menuChanged(false))
  state.reduce(.pointerMoved)
  #expect(state.visibility == .visible)
}

@Test func cleanWritingPaneClosesDirectlyAndDirtyPaneUsesWritingCopy() {
  #expect(
    chromePolicy.closeDisposition(fileName: "essay.md", dirty: false) == .closeImmediately)
  #expect(
    chromePolicy.closeDisposition(fileName: "essay.md", dirty: true)
      == .confirm(title: "Close essay.md?", message: "Unsaved changes will be lost."))
}

@Test func nativeTextShortcutsMapOnlyBoldAndItalicWithCommand() {
  #expect(WritingChromePolicy.formatShortcut(key: "b", command: true) == .bold)
  #expect(WritingChromePolicy.formatShortcut(key: "B", command: true) == .bold)
  #expect(WritingChromePolicy.formatShortcut(key: "i", command: true) == .italic)
  #expect(WritingChromePolicy.formatShortcut(key: "k", command: true) == nil)
  #expect(WritingChromePolicy.formatShortcut(key: "b", command: false) == nil)
}

@Test func everyChromeActionNamesItselfAndItsAccessibilityElement() {
  for action in WritingChromeAction.allCases {
    #expect(!action.title().isEmpty, "\(action)")
    #expect(!action.systemImage.isEmpty, "\(action)")
    #expect(action.accessibilityIdentifier(paneId: "p1").hasSuffix("-p1"), "\(action)")
  }
  let identifiers = WritingChromeAction.allCases
    .filter { $0 != .useReadingWidth }
    .map { $0.accessibilityIdentifier(paneId: "p1") }
  #expect(Set(identifiers).count == identifiers.count)
  #expect(WritingChromeAction.focusMode.title() == "Focus mode")
  #expect(WritingChromeAction.focusMode.title(focusMode: true) == "Turn off Focus mode")
  #expect(
    WritingChromeAction.useFullWidth.accessibilityIdentifier(paneId: "p1")
      == WritingChromeAction.useReadingWidth.accessibilityIdentifier(paneId: "p1"))
}

@Test func theSelectionToolbarIsDrawnFromTheFormatsOfItsActions() {
  let actions = chromePolicy.plan(kind: .text, hasSelection: true).selectionActions
  for action in actions {
    let routes = [
      action.directFormat != nil, !action.formatChoices.isEmpty, action.destinationFormat != nil,
    ]
    #expect(routes.filter { $0 }.count == 1, "\(action)")
  }
  #expect(
    WritingChromeAction.heading.formatChoices.map(\.kind) == [.heading1, .heading2, .heading3])
  #expect(WritingChromeAction.list.formatChoices.map(\.kind) == [.bullet_list, .numbered_list])
  #expect(WritingChromeAction.link.shortcutKey == "k")
  #expect(WritingFormatKind.image.destinationPrompt?.title == "Image path")
  #expect(WritingFormatKind.link.destinationPrompt?.placeholder == "https://")
  #expect(WritingFormatKind.bold.destinationPrompt == nil)
  for action in [WritingChromeAction.bold, .italic] {
    let key = String(action.shortcutKey ?? " ")
    #expect(WritingChromePolicy.formatShortcut(key: key, command: true) == action.directFormat)
  }
}

@Test func theChromeFadesByTheMetricsOfItsVisibility() {
  let metrics = WritingChromeMetrics.standard
  #expect(metrics.opacity(for: .visible) == 1)
  #expect(metrics.opacity(for: .dimmed) == metrics.dimmedOpacity)
  #expect(metrics.fadeDuration(into: .dimmed) == metrics.dimDuration)
  #expect(metrics.fadeDuration(into: .visible) == metrics.revealDuration)
}

@Test func aRailDividerRunsAlongTheRightEdgeOfItsRail() {
  let rail = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 10, y: 20), size: PaneLayoutSize(width: 200, height: 300))
  let divider = WritingLayoutMetrics.standard.railDivider(after: rail)
  #expect(divider.origin.x == 210)
  #expect(divider.origin.y == 20)
  #expect(divider.size.width == WritingLayoutMetrics.standard.railDividerWidth)
  #expect(divider.size.height == 300)
}
