import Foundation
import Testing

@testable import SoyehtClient

private let phoneFonts = PaneLayoutFontMetrics(
  terminal: PaneLayoutFontMetric(characterWidth: 8, lineHeight: 18, pointSize: 13),
  editor: PaneLayoutFontMetric(characterWidth: 6.9, lineHeight: 19, pointSize: 11.5),
  chrome: PaneLayoutFontMetric(characterWidth: 7.2, lineHeight: 17, pointSize: 12),
  agent: PaneLayoutFontMetric(characterWidth: 9, lineHeight: 21, pointSize: 15),
  prose: PaneLayoutFontMetric(characterWidth: 11.037, lineHeight: 32, pointSize: 20),
  screenplay: PaneLayoutFontMetric(characterWidth: 9.59375, lineHeight: 27, pointSize: 16))

private let phoneFloors = PaneLayoutFloorFormula.surfaceFloors(fonts: phoneFonts)
private let phonePolicy = PhoneWritingLayoutPolicy(floors: phoneFloors)
private let phoneChrome = PhoneWritingChromePolicy()
private let phoneSheets = PhoneWritingSheetPolicy()
private let phoneSafeArea = PhoneWritingViewportInsets(top: 50, bottom: 34)

@Test func phoneWritingChromeHasOneFixedSingleLineHeaderAndCompleteInventories() {
  let text = phoneChrome.plan(kind: .text, hasSelection: true)
  let screenplay = phoneChrome.plan(kind: .screenplay)
  let presentation = phoneChrome.plan(kind: .presentation)

  #expect(text.headerHeight == 44)
  #expect(text.minimumActionTarget == 44)
  #expect(text.splitKinds == [.text, .screenplay, .presentation, .agent])
  #expect(
    text.menuGroups == [
      [.font, .useFullWidth, .focusMode], [.splitColumns, .splitRows], [.closePane],
    ])
  #expect(
    text.selectionActions == [.bold, .italic, .heading, .list, .quote, .link, .image])
  #expect(
    screenplay.menuGroups == [
      [.ideaBoard, .scenes, .focusMode], [.splitColumns, .splitRows], [.closePane],
    ])
  #expect(
    presentation.menuGroups == [[.present], [.splitColumns, .splitRows], [.closePane]])
}

@Test func phoneWritingSheetsShareOneActionTargetAndNamedDetentInventory() {
  for kind in PhoneWritingSheetKind.allCases {
    #expect(phoneSheets.plan(for: kind).minimumActionTarget == 44)
  }
  #expect(phoneSheets.plan(for: .scenes).detents == [.medium])
  #expect(phoneSheets.plan(for: .ideaBoard).detents == [.medium])
  #expect(phoneSheets.plan(for: .split).detents == [.medium, .large])
  #expect(phoneSheets.plan(for: .link).detents == [.medium])
  #expect(phoneSheets.plan(for: .image).detents == [.medium])
}

@Test func screenplayElementOverlayConvertsCaretAtOneAndTwoTimesWithoutScalingTheTarget() {
  let policy = PhoneScreenplayElementOverlayPolicy(minimumTarget: 44, clearance: 3)
  let caret = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 20, y: 30), size: PaneLayoutSize(width: 2, height: 18))
  let origin = PaneLayoutPoint(x: 5, y: 7)
  let visible = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 0, y: 0), size: PaneLayoutSize(width: 375, height: 500))

  let one = policy.caretRectInScroll(caretRect: caret, pageOrigin: origin, zoomScale: 1)
  let two = policy.caretRectInScroll(caretRect: caret, pageOrigin: origin, zoomScale: 2)
  #expect(
    one
      == PaneLayoutRect(
        origin: PaneLayoutPoint(x: 25, y: 37), size: PaneLayoutSize(width: 2, height: 18)))
  #expect(
    two
      == PaneLayoutRect(
        origin: PaneLayoutPoint(x: 45, y: 67), size: PaneLayoutSize(width: 4, height: 36)))
  #expect(
    policy.buttonRect(for: one, visibleRect: visible)?.size == PaneLayoutSize(width: 44, height: 44)
  )
  #expect(
    policy.buttonRect(for: two, visibleRect: visible)?.size == PaneLayoutSize(width: 44, height: 44)
  )
  #expect(
    policy.buttonRect(
      for: PaneLayoutRect(
        origin: PaneLayoutPoint(x: 500, y: 700), size: PaneLayoutSize(width: 2, height: 18)),
      visibleRect: visible) == nil)
}

@Test func phoneWritingChromeUsesAcceptedMetricsAndHonorsReduceMotion() {
  let metrics = PhoneWritingLayoutMetrics.standard

  #expect(metrics.headerHorizontalInset == 14)
  #expect(metrics.headerNamePointSize == 14)
  #expect(metrics.headerCountPointSize == 14)
  #expect(metrics.headerKindIconSize == 18)
  #expect(metrics.headerActionIconSize == 17)
  #expect(metrics.screenplayInlineMenuClearance == 3)
  #expect(phoneChrome.duration(for: .dim, reduceMotion: false) == 0.16)
  #expect(phoneChrome.duration(for: .reveal, reduceMotion: false) == 0.10)
  #expect(phoneChrome.duration(for: .dim, reduceMotion: true) == 0)
  #expect(phoneChrome.duration(for: .reveal, reduceMotion: true) == 0)
}

@Test func phoneWritingBodyGeometryDoesNotAddOrTranslateAMacHeader() throws {
  let bodyMetrics = PhoneWritingLayoutMetrics.standard.bodyWritingMetrics()
  let bodyFloors = PhoneWritingLayoutMetrics.standard.bodySurfaceFloors(from: phoneFloors)
  let textFloor = try #require(phoneFloors[.text])
  let policy = WritingLayoutPolicy(
    metrics: bodyMetrics, floors: bodyFloors,
    gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))
  let plan = try policy.layout(
    WritingLayoutRequest(
      kind: .text, viewport: PaneLayoutSize(width: 375, height: 394),
      font: phoneFonts.prose))

  #expect(plan.headerRect.size.height == 0)
  #expect(plan.contentRect.origin.y == bodyMetrics.paneInset)
  #expect(plan.contentSize.height == 394)
  #expect(plan.overflowAxes.isEmpty)
  #expect(
    bodyFloors[.text]?.height
      == textFloor.height - WritingLayoutMetrics.standard.headerHeight)
}

@Test func compactPhonePresentationStartsImmediatelyBelowItsNumberStrip() throws {
  let bodyMetrics = PhoneWritingLayoutMetrics.standard.bodyWritingMetrics()
  let bodyFloors = PhoneWritingLayoutMetrics.standard.bodySurfaceFloors(from: phoneFloors)
  let policy = WritingLayoutPolicy(
    metrics: bodyMetrics, floors: bodyFloors,
    gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))
  let plan = try policy.layout(
    WritingLayoutRequest(
      kind: .presentation, viewport: PaneLayoutSize(width: 375, height: 812),
      font: phoneFonts.prose))
  guard case .presentation(let presentation) = plan.surface else {
    Issue.record("Expected presentation plan")
    return
  }

  #expect(presentation.navigation == .numbers)
  #expect(
    presentation.stageRect.origin.y
      == presentation.navigationRect.maxY + bodyMetrics.paneInset)
  #expect(
    abs(presentation.stageRect.size.width / presentation.stageRect.size.height - 16.0 / 9.0)
      < 0.000_001)
}

@Test func phoneProjectionKeepsCanonicalWritingOrderAndOnlyExplicitCompanions() {
  let tree = writingFixtureTree()
  let plan = phonePolicy.layout(
    request(tree: tree, selectedPaneId: "screenplay-agent"))

  #expect(plan.leaves.map(\.identity.paneId) == ["text", "screenplay", "presentation"])
  #expect(plan.leaves[0].companionAgentPaneId == nil)
  #expect(plan.leaves[1].companionAgentPaneId == "screenplay-agent")
  #expect(plan.leaves[2].companionAgentPaneId == nil)
  #expect(plan.leaves[1].expanded)
  #expect(plan.leaves[1].activeTab == .agent)
  #expect(!plan.leaves[0].expanded)
  #expect(!plan.leaves[2].expanded)
}

@Test func phoneInitialStateOpensSelectedWritingPaneOrFirstCanonicalPane() {
  let tree = writingFixtureTree()
  let selected = phonePolicy.layout(request(tree: tree, selectedPaneId: "presentation"))
  let noSelection = phonePolicy.layout(request(tree: tree))
  let unrelatedSelection = phonePolicy.layout(request(tree: tree, selectedPaneId: "terminal"))

  #expect(selected.leaves.map(\.expanded) == [false, false, true])
  #expect(noSelection.leaves.map(\.expanded) == [true, false, false])
  #expect(unrelatedSelection.leaves.map(\.expanded) == [true, false, false])
}

@Test func storedAllCollapsedStateWinsOverSelectionAndPrunesDeadIdentities() {
  let first = phonePolicy.layout(request(tree: writingFixtureTree(), selectedPaneId: "text"))
  let stored = PhoneWritingLocalState(
    panes: first.localState.panes.map {
      PhoneWritingPaneState(identity: $0.identity, expanded: false)
    } + [
      PhoneWritingPaneState(
        identity: PhoneWritingIdentity(paneId: "closed", fileId: "closed-file"),
        expanded: true)
    ])
  let restored = phonePolicy.layout(
    request(
      tree: writingFixtureTree(), selectedPaneId: "presentation", localState: stored))

  #expect(restored.leaves.allSatisfy { !$0.expanded })
  #expect(
    restored.localState.panes.map(\.identity.paneId) == [
      "text", "screenplay", "presentation",
    ])
}

@Test func aChangedFileIdentityStartsCollapsedWithoutBorrowingTheOldDocumentState() {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "text", kind: .text))
  let old = PhoneWritingLocalState(
    panes: [
      PhoneWritingPaneState(
        identity: PhoneWritingIdentity(paneId: "text", fileId: "old-file"), expanded: true)
    ])
  let plan = phonePolicy.layout(
    PhoneWritingLayoutRequest(
      tree: tree,
      documents: [document("text", kind: .text, fileId: "new-file")],
      selectedPaneId: "text", localState: old,
      viewport: PaneLayoutSize(width: 375, height: 812), safeArea: phoneSafeArea))

  #expect(plan.leaves[0].identity.fileId == "new-file")
  #expect(!plan.leaves[0].expanded)
}

@Test func transientMissingDocumentsPreserveStateWithoutSavingOverIt() {
  let tree = writingFixtureTree()
  let seed = phonePolicy.layout(request(tree: tree, selectedPaneId: "screenplay-agent"))
  var stored = PhoneWritingLocalState(
    panes: seed.localState.panes.map { pane in
      var pane = pane
      pane.expanded = true
      if pane.identity.paneId == "screenplay" {
        pane.activeTab = .agent
        pane.screenplayZoom = 2.25
      }
      return pane
    })
  let expected = stored
  var saves = 0

  let phases:
    [(tree: PaneLayoutTree?, documents: [PhoneWritingDocumentDescriptor], authoritative: Bool)] = [
      (nil, [], false),
      (tree, [], true),
      (tree, fixtureDocuments(), true),
    ]
  for phase in phases {
    let plan = phonePolicy.layout(
      PhoneWritingLayoutRequest(
        tree: phase.tree, documents: phase.documents,
        selectedPaneId: "screenplay-agent", localState: stored,
        snapshotIsAuthoritative: phase.authoritative,
        viewport: PaneLayoutSize(width: 375, height: 812), safeArea: phoneSafeArea))
    if plan.localState != stored {
      stored = plan.localState
      saves += 1
    }
    if phase.tree != nil {
      #expect(plan.leaves.map(\.expanded) == [true, true, true])
      #expect(plan.leaves[1].activeTab == .agent)
    } else {
      #expect(plan.leaves.isEmpty)
    }
  }

  #expect(stored == expected)
  #expect(saves == 0)
}

@Test func localStateRoundTripsAndUnknownVersionsBecomeAbsentState() throws {
  let state = PhoneWritingLocalState(
    panes: [
      PhoneWritingPaneState(
        identity: PhoneWritingIdentity(paneId: "screenplay", fileId: "script"),
        expanded: true, activeTab: .agent, screenplayZoom: 2.5)
    ])
  let encoded = try state.encoded()

  #expect(PhoneWritingLocalState.decoded(encoded) == state)
  #expect(
    PhoneWritingLocalState.decoded(
      Data("{\"version\":99,\"panes\":[]}".utf8)) == nil)
  #expect(PhoneWritingLocalState.decoded(Data("not json".utf8)) == nil)

  let directUnknown = phonePolicy.layout(
    request(
      tree: writingFixtureTree(), selectedPaneId: "presentation",
      localState: PhoneWritingLocalState(version: 99)))
  #expect(directUnknown.leaves.map(\.expanded) == [false, false, true])
}

@Test func stateActionsArePureBoundedAndClosedByKindAndPair() throws {
  let initial = phonePolicy.layout(
    request(tree: writingFixtureTree(), selectedPaneId: "screenplay"))
  let screenplay = try #require(
    initial.leaves.first { $0.identity.paneId == "screenplay" })
  let text = try #require(initial.leaves.first { $0.identity.paneId == "text" })

  let agentSelected = phonePolicy.applying(
    .selectTab(screenplay.identity, .agent), to: initial)
  let zoomedPlan = phonePolicy.layout(
    request(tree: writingFixtureTree(), localState: agentSelected))
  let zoomedState = phonePolicy.applying(
    .setScreenplayZoom(screenplay.identity, 99), to: zoomedPlan)
  let ignoredTextZoom = phonePolicy.applying(
    .setScreenplayZoom(text.identity, 2), to: zoomedPlan)
  let ignoredAgentTab = phonePolicy.applying(
    .selectTab(text.identity, .agent), to: zoomedPlan)

  #expect(agentSelected.panes.first { $0.identity == screenplay.identity }?.activeTab == .agent)
  #expect(agentSelected.panes.first { $0.identity == screenplay.identity }?.expanded == true)
  #expect(zoomedState.panes.first { $0.identity == screenplay.identity }?.screenplayZoom == 4)
  #expect(ignoredTextZoom.panes.first { $0.identity == text.identity }?.screenplayZoom == 1)
  #expect(ignoredAgentTab.panes.first { $0.identity == text.identity }?.activeTab == .document)
}

@Test func onlyExplicitRemotePhoneActionMovesSharedEngineFocus() {
  var selectedPaneIds: [String] = []
  let localActions: [PhoneWritingFocusAction] = [
    .enterWorkspace, .toggleLeaf, .selectLocalTab, .restoreLocalState,
  ]

  for action in localActions {
    PhoneWritingFocusPolicy.apply(action) { selectedPaneIds.append($0) }
  }
  #expect(selectedPaneIds.isEmpty)

  PhoneWritingFocusPolicy.apply(.sendToAgent("agent")) {
    selectedPaneIds.append($0)
  }
  #expect(selectedPaneIds == ["agent"])
}

@Test func invalidOrInferredCompanionsNeverCreateAnAgentTab() {
  let inferred = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "text", kind: .text))),
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "agent", kind: .agent))),
      ]))
  let invalid = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "text", kind: .text))),
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
      ],
      companion: PaneLayoutCompanion(writingPaneId: "text", agentPaneId: "terminal")))

  for tree in [inferred, invalid] {
    let plan = phonePolicy.layout(request(tree: tree))
    #expect(plan.leaves.count == 1)
    #expect(plan.leaves[0].companionAgentPaneId == nil)
    #expect(plan.leaves[0].documentTabRect == nil)
    #expect(plan.leaves[0].agentTabRect == nil)
  }
}

@Test func missingAndInconsistentDocumentsStayVisibleAsNamedStates() {
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .vertical,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "missing", kind: .text))),
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "wrong", kind: .screenplay))),
      ]))
  let plan = phonePolicy.layout(
    PhoneWritingLayoutRequest(
      tree: tree,
      documents: [document("wrong", kind: .text)],
      paneNames: ["missing": "Draft without a loaded document"],
      viewport: PaneLayoutSize(width: 375, height: 812), safeArea: phoneSafeArea))

  #expect(plan.leaves.map(\.availability) == [.missingDocument, .inconsistentDocument])
  #expect(plan.leaves.map(\.identity.paneId) == ["missing", "wrong"])
  #expect(plan.leaves[0].name == "Draft without a loaded document")
  #expect(plan.leaves[0].name != plan.leaves[0].identity.paneId)
}

@Test func headerCarriesTheCompleteNameAndEveryControlMeetsTheHitTarget() throws {
  let fullName = "A complete screenplay title that the phone may truncate only while drawing"
  let documents = fixtureDocuments().map { descriptor in
    descriptor.paneId == "screenplay"
      ? PhoneWritingDocumentDescriptor(
        paneId: descriptor.paneId, fileId: descriptor.fileId, name: fullName,
        kind: descriptor.kind, font: descriptor.font)
      : descriptor
  }
  let plan = phonePolicy.layout(
    PhoneWritingLayoutRequest(
      tree: writingFixtureTree(), documents: documents,
      selectedPaneId: "screenplay", viewport: PaneLayoutSize(width: 375, height: 812),
      safeArea: phoneSafeArea))
  let leaf = try #require(plan.leaves.first { $0.identity.paneId == "screenplay" })

  #expect(leaf.name == fullName)
  #expect(leaf.headerRect.size.height == 88)
  #expect(leaf.documentTabRect?.size.height == 44)
  #expect(leaf.agentTabRect?.size.height == 44)
  #expect((leaf.documentTabRect?.size.width ?? 0) >= 44)
  #expect((leaf.agentTabRect?.size.width ?? 0) >= 44)
}

@Test func planCarriesTheValidatedFontAndTextWidthPreferenceForTheView() throws {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "text", kind: .text))
  let descriptor = PhoneWritingDocumentDescriptor(
    paneId: "text", fileId: "file-text", name: "Text", kind: .text,
    font: phoneFonts.prose, fullWidth: true)
  let plan = phonePolicy.layout(
    PhoneWritingLayoutRequest(
      tree: tree, documents: [descriptor],
      viewport: PaneLayoutSize(width: 375, height: 812), safeArea: phoneSafeArea))
  let leaf = try #require(plan.leaves.first)

  #expect(leaf.font == phoneFonts.prose)
  #expect(leaf.fullWidth)
  #expect(leaf.screenplayPageSize == nil)
}

@Test func screenplayPageIsFixedRatioAndFitsTheShorterViewportAxis() throws {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "screenplay", kind: .screenplay))
  let plan = phonePolicy.layout(
    request(
      tree: tree, selectedPaneId: "screenplay",
      viewport: PaneLayoutSize(width: 800, height: 375)))
  let leaf = try #require(plan.leaves.first)
  let page = try #require(leaf.screenplayPageSize)
  let content = try #require(leaf.contentRect)
  let fit = try #require(leaf.screenplayFitScale)

  #expect(page.width == 61 * phoneFonts.screenplay.characterWidth + 2 * 74)
  #expect(abs(page.height / page.width - 11.0 / 8.5) < 0.000_001)
  #expect(page.width * fit <= content.size.width + 0.000_001)
  #expect(page.height * fit <= content.size.height + 0.000_001)
  #expect(abs(page.height * fit - content.size.height) < 0.000_001)
}

@Test func phonePresentationKeepsSixteenByNineAndSwitchesNavigationByMeasuredWidth() throws {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "presentation", kind: .presentation))
  let writingPolicy = WritingLayoutPolicy(
    floors: phoneFloors, gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))

  for viewport in [
    PaneLayoutSize(width: 375, height: 812),
    PaneLayoutSize(width: 390, height: 667),
    PaneLayoutSize(width: 430, height: 568),
  ] {
    let phone = phonePolicy.layout(
      request(tree: tree, selectedPaneId: "presentation", viewport: viewport))
    let leaf = try #require(phone.leaves.first)
    let content = try #require(leaf.contentRect)
    let plan = try writingPolicy.layout(
      WritingLayoutRequest(
        kind: .presentation,
        viewport: PaneLayoutSize(
          width: content.size.width,
          height: content.size.height + WritingLayoutMetrics.standard.headerHeight),
        font: phoneFonts.prose))
    guard case .presentation(let presentation) = plan.surface else {
      Issue.record("Expected a presentation plan")
      continue
    }
    #expect(presentation.navigation == .numbers)
    #expect(
      abs(presentation.stageRect.size.width / presentation.stageRect.size.height - 16.0 / 9.0)
        < 0.000_001)
  }

  let landscape = phonePolicy.layout(
    request(
      tree: tree, selectedPaneId: "presentation",
      viewport: PaneLayoutSize(width: 812, height: 375)))
  let content = try #require(landscape.leaves.first?.contentRect)
  let landscapePlan = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .presentation,
      viewport: PaneLayoutSize(
        width: content.size.width,
        height: content.size.height + WritingLayoutMetrics.standard.headerHeight),
      font: phoneFonts.prose))
  guard case .presentation(let presentation) = landscapePlan.surface else {
    Issue.record("Expected a landscape presentation plan")
    return
  }
  #expect(presentation.navigation == .thumbnails)
}

@Test func requestedPhoneMatrixNeverUsesHorizontalOverflowOrShrinksTargets() throws {
  let widths = [375.0, 390, 430]
  let heights = [812.0, 667, 568, 430, 375]
  let keyboardHeights = [0.0, 291]
  let seed = phonePolicy.layout(request(tree: writingFixtureTree()))
  let allExpanded = PhoneWritingLocalState(
    panes: seed.localState.panes.map {
      PhoneWritingPaneState(identity: $0.identity, expanded: true)
    })

  for width in widths {
    for height in heights {
      for keyboard in keyboardHeights {
        let plan = phonePolicy.layout(
          request(
            tree: writingFixtureTree(), localState: allExpanded,
            viewport: PaneLayoutSize(width: width, height: height),
            obscuredBottom: keyboard))
        #expect(plan.isAtLeastMinimumExercisedWidth)
        #expect(!plan.overflowAxes.contains(.horizontal))
        #expect(plan.contentSize.width == width)
        #expect(
          plan.leaves.map(\.identity.paneId) == [
            "text", "screenplay", "presentation",
          ])
        for leaf in plan.leaves {
          #expect(leaf.headerRect.size.width == width - 30)
          #expect(leaf.headerRect.size.height >= 44)
          #expect(leaf.contentRect?.size.width == width - 30)
          #expect(leaf.contentRect?.size.height ?? 0 > 0)
        }
        let screenplay = try #require(
          plan.leaves.first { $0.identity.paneId == "screenplay" })
        let scale = try #require(screenplay.screenplayFitScale)
        let pageSize = try #require(screenplay.screenplayPageSize)
        #expect(scale >= WritingLayoutMetrics.standard.screenplayMinimumScale)
        let basePageWidth = 61 * phoneFonts.screenplay.characterWidth + 2 * 74
        #expect(pageSize.width == basePageWidth)
        #expect(abs(pageSize.height / pageSize.width - 11.0 / 8.5) < 0.000_001)
        #expect(basePageWidth * scale <= (screenplay.contentRect?.size.width ?? 0) + 0.000_001)
        #expect(
          pageSize.height * scale <= (screenplay.contentRect?.size.height ?? 0) + 0.000_001)
      }
    }
  }
}

@Test func onePointBelowTheAcceptedPhoneWidthIsNamedInsteadOfSilentlyAccepted() {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "text", kind: .text))
  let accepted = phonePolicy.layout(
    request(tree: tree, viewport: PaneLayoutSize(width: 375, height: 812)))
  let onePointShort = phonePolicy.layout(
    request(tree: tree, viewport: PaneLayoutSize(width: 374, height: 812)))

  #expect(accepted.isAtLeastMinimumExercisedWidth)
  #expect(!onePointShort.isAtLeastMinimumExercisedWidth)
}

@Test func keyboardPressureScrollsTheScreenplayLeafInsteadOfShrinkingIt() throws {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "screenplay", kind: .screenplay))
  let plan = phonePolicy.layout(
    request(
      tree: tree, selectedPaneId: "screenplay",
      viewport: PaneLayoutSize(width: 375, height: 812), obscuredBottom: 291))
  let leaf = try #require(plan.leaves.first)

  #expect(leaf.expanded)
  #expect(
    leaf.contentRect?.size.height
      == phoneFloors[.screenplay]!.height - WritingLayoutMetrics.standard.headerHeight)
  #expect(plan.overflowAxes == [.vertical])
  let hiddenRowExtent = max(
    0,
    leaf.rowRect.origin.y + leaf.rowRect.size.height
      - (plan.viewportRect.origin.y + plan.viewportRect.size.height))
  #expect(plan.contentSize.height - 812 >= hiddenRowExtent)
}

private func request(
  tree: PaneLayoutTree, selectedPaneId: String? = nil,
  localState: PhoneWritingLocalState? = nil,
  viewport: PaneLayoutSize = PaneLayoutSize(width: 375, height: 812),
  obscuredBottom: Double = 0
) -> PhoneWritingLayoutRequest {
  PhoneWritingLayoutRequest(
    tree: tree, documents: fixtureDocuments(), selectedPaneId: selectedPaneId,
    localState: localState, viewport: viewport, safeArea: phoneSafeArea,
    obscuredBottom: obscuredBottom)
}

private func writingFixtureTree() -> PaneLayoutTree {
  .split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "text", kind: .text))),
        PaneLayoutBranch(
          tree: .split(
            PaneLayoutSplit(
              axis: .vertical,
              children: [
                PaneLayoutBranch(
                  tree: .leaf(PaneLayoutLeaf(paneId: "screenplay", kind: .screenplay))),
                PaneLayoutBranch(
                  tree: .leaf(PaneLayoutLeaf(paneId: "screenplay-agent", kind: .agent))),
              ],
              companion: PaneLayoutCompanion(
                writingPaneId: "screenplay", agentPaneId: "screenplay-agent")))),
        PaneLayoutBranch(
          tree: .leaf(PaneLayoutLeaf(paneId: "presentation", kind: .presentation))),
        PaneLayoutBranch(
          tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
      ]))
}

private func fixtureDocuments() -> [PhoneWritingDocumentDescriptor] {
  [
    document("text", kind: .text),
    document("screenplay", kind: .screenplay),
    document("presentation", kind: .presentation),
  ]
}

private func document(
  _ paneId: String, kind: PaneLayoutKind, fileId: String? = nil
) -> PhoneWritingDocumentDescriptor {
  PhoneWritingDocumentDescriptor(
    paneId: paneId, fileId: fileId ?? "file-\(paneId)", name: "Name \(paneId)", kind: kind,
    font: kind == .screenplay ? phoneFonts.screenplay : phoneFonts.prose)
}
