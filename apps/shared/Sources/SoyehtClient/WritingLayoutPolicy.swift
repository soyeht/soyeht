import Foundation

public enum WritingTextWidthMode: Equatable, Sendable {
  case reading
  case fullWidth
}

public enum WritingSceneNavigationMode: Equatable, Sendable {
  case rail
  case drawer
}

public enum WritingSlideNavigationMode: Equatable, Sendable {
  case thumbnails
  case numbers
}

public enum WritingCompanionPresentation: Equatable, Sendable {
  case sideBySide
  case below
  case tabs

  public var splitAxis: PaneLayoutAxis? {
    switch self {
    case .sideBySide: .horizontal
    case .below: .vertical
    case .tabs: nil
    }
  }
}

public struct WritingLayoutMetrics: Equatable, Sendable {
  public var headerHeight: Double
  public var paneInset: Double
  public var stackSpacing: Double

  public var textToolbarHeight: Double
  public var textReviewHeight: Double
  public var textPageInset: Double
  public var textReadingCharacters: Double
  public var textCompactCharacters: Double
  public var textCompactLines: Double

  public var screenplayToolbarHeight: Double
  public var screenplaySmartTypeHeight: Double
  public var screenplayColumns: Double
  public var screenplayPageHorizontalInset: Double
  public var screenplayPageRatio: Double
  public var screenplayMinimumScale: Double
  public var screenplaySceneRailWidth: Double
  public var railDividerWidth: Double

  public var presentationThumbnailRailWidth: Double
  public var presentationNumericRailHeight: Double
  public var presentationFooterHeight: Double
  public var presentationMinimumStage: PaneLayoutSize
  public var presentationMaximumStage: PaneLayoutSize

  public var companionTabStripHeight: Double

  public init(
    headerHeight: Double, paneInset: Double, stackSpacing: Double,
    textToolbarHeight: Double, textReviewHeight: Double, textPageInset: Double,
    textReadingCharacters: Double, textCompactCharacters: Double, textCompactLines: Double,
    screenplayToolbarHeight: Double, screenplaySmartTypeHeight: Double,
    screenplayColumns: Double, screenplayPageHorizontalInset: Double,
    screenplayPageRatio: Double, screenplayMinimumScale: Double,
    screenplaySceneRailWidth: Double, railDividerWidth: Double,
    presentationThumbnailRailWidth: Double, presentationNumericRailHeight: Double,
    presentationFooterHeight: Double, presentationMinimumStage: PaneLayoutSize,
    presentationMaximumStage: PaneLayoutSize, companionTabStripHeight: Double
  ) {
    self.headerHeight = headerHeight
    self.paneInset = paneInset
    self.stackSpacing = stackSpacing
    self.textToolbarHeight = textToolbarHeight
    self.textReviewHeight = textReviewHeight
    self.textPageInset = textPageInset
    self.textReadingCharacters = textReadingCharacters
    self.textCompactCharacters = textCompactCharacters
    self.textCompactLines = textCompactLines
    self.screenplayToolbarHeight = screenplayToolbarHeight
    self.screenplaySmartTypeHeight = screenplaySmartTypeHeight
    self.screenplayColumns = screenplayColumns
    self.screenplayPageHorizontalInset = screenplayPageHorizontalInset
    self.screenplayPageRatio = screenplayPageRatio
    self.screenplayMinimumScale = screenplayMinimumScale
    self.screenplaySceneRailWidth = screenplaySceneRailWidth
    self.railDividerWidth = railDividerWidth
    self.presentationThumbnailRailWidth = presentationThumbnailRailWidth
    self.presentationNumericRailHeight = presentationNumericRailHeight
    self.presentationFooterHeight = presentationFooterHeight
    self.presentationMinimumStage = presentationMinimumStage
    self.presentationMaximumStage = presentationMaximumStage
    self.companionTabStripHeight = companionTabStripHeight
  }

  public static let standard = WritingLayoutMetrics(
    headerHeight: PaneLayoutContentMetrics.writingHeaderHeight,
    paneInset: PaneLayoutContentMetrics.paneCardInset,
    stackSpacing: PaneLayoutContentMetrics.paneStackSpacing,
    textToolbarHeight: PaneLayoutContentMetrics.textToolbarHeight,
    textReviewHeight: PaneLayoutContentMetrics.textReviewHeight,
    textPageInset: PaneLayoutContentMetrics.textPageInset,
    textReadingCharacters: PaneLayoutContentMetrics.textReadingCharacters,
    textCompactCharacters: PaneLayoutFloorFormula.compactTextCharacters,
    textCompactLines: PaneLayoutContentMetrics.textCompactLines,
    screenplayToolbarHeight: PaneLayoutContentMetrics.screenplayToolbarHeight,
    screenplaySmartTypeHeight: PaneLayoutContentMetrics.screenplaySmartTypeHeight,
    screenplayColumns: PaneLayoutFloorFormula.screenplayColumns,
    screenplayPageHorizontalInset: PaneLayoutContentMetrics.screenplayPageHorizontalInset,
    screenplayPageRatio: PaneLayoutContentMetrics.screenplayPageRatio,
    screenplayMinimumScale: PaneLayoutContentMetrics.screenplayMinimumScale,
    screenplaySceneRailWidth: PaneLayoutContentMetrics.screenplaySceneRailWidth,
    railDividerWidth: PaneLayoutContentMetrics.writingRailDividerWidth,
    presentationThumbnailRailWidth: PaneLayoutContentMetrics.presentationThumbnailRailWidth,
    presentationNumericRailHeight: PaneLayoutContentMetrics.presentationNumericRailHeight,
    presentationFooterHeight: PaneLayoutContentMetrics.presentationFooterHeight,
    presentationMinimumStage: PaneLayoutContentMetrics.presentationStage,
    presentationMaximumStage: PaneLayoutContentMetrics.presentationMaximumStage,
    companionTabStripHeight: PaneLayoutContentMetrics.companionTabStripHeight)

  func surfaceFloor(
    kind: PaneLayoutKind, font: PaneLayoutFontMetric
  ) -> PaneLayoutSize? {
    switch kind {
    case .text:
      return PaneLayoutSize(
        width: ceil(
          textCompactCharacters * font.characterWidth + 2 * textPageInset + 2 * paneInset),
        height: ceil(
          2 * paneInset + headerHeight + textToolbarHeight + textReviewHeight
            + 3 * stackSpacing + 2 * textPageInset + textCompactLines * font.lineHeight))
    case .screenplay:
      let basePageWidth =
        screenplayColumns * font.characterWidth + 2 * screenplayPageHorizontalInset
      let compactPageWidth = ceil(basePageWidth * screenplayMinimumScale)
      return PaneLayoutSize(
        width: compactPageWidth,
        height: headerHeight + screenplayToolbarHeight + screenplaySmartTypeHeight
          + ceil(compactPageWidth * screenplayPageRatio))
    case .presentation:
      return PaneLayoutSize(
        width: presentationMinimumStage.width + 2 * paneInset,
        height: headerHeight + presentationNumericRailHeight
          + presentationMinimumStage.height + presentationFooterHeight + 2 * paneInset)
    default:
      return nil
    }
  }
}

public struct WritingTextLayoutPlan: Equatable, Sendable {
  public var mode: WritingTextWidthMode
  /// The text-container rectangle. Page and pane insets are outside this rectangle.
  public var columnRect: PaneLayoutRect
  public var visibleCharacters: Double

  public init(
    mode: WritingTextWidthMode, columnRect: PaneLayoutRect, visibleCharacters: Double
  ) {
    self.mode = mode
    self.columnRect = columnRect
    self.visibleCharacters = visibleCharacters
  }
}

public struct WritingScreenplayLayoutPlan: Equatable, Sendable {
  public var navigation: WritingSceneNavigationMode
  public var navigationRect: PaneLayoutRect?
  /// The fixed Courier page before uniform display scaling.
  public var pageSize: PaneLayoutSize
  public var pageRect: PaneLayoutRect
  public var scale: Double

  public init(
    navigation: WritingSceneNavigationMode, navigationRect: PaneLayoutRect?,
    pageSize: PaneLayoutSize, pageRect: PaneLayoutRect, scale: Double
  ) {
    self.navigation = navigation
    self.navigationRect = navigationRect
    self.pageSize = pageSize
    self.pageRect = pageRect
    self.scale = scale
  }
}

public struct WritingPresentationLayoutPlan: Equatable, Sendable {
  public var navigation: WritingSlideNavigationMode
  public var navigationRect: PaneLayoutRect
  public var stageRect: PaneLayoutRect
  public var footerRect: PaneLayoutRect
  /// Scale relative to the accepted 320×180 minimum stage.
  public var scale: Double

  public init(
    navigation: WritingSlideNavigationMode, navigationRect: PaneLayoutRect,
    stageRect: PaneLayoutRect, footerRect: PaneLayoutRect, scale: Double
  ) {
    self.navigation = navigation
    self.navigationRect = navigationRect
    self.stageRect = stageRect
    self.footerRect = footerRect
    self.scale = scale
  }
}

public enum WritingSurfaceLayout: Equatable, Sendable {
  case text(WritingTextLayoutPlan)
  case screenplay(WritingScreenplayLayoutPlan)
  case presentation(WritingPresentationLayoutPlan)
}

public struct WritingLayoutPlan: Equatable, Sendable {
  public var headerRect: PaneLayoutRect
  public var toolbarRect: PaneLayoutRect?
  public var reviewRect: PaneLayoutRect?
  public var contentRect: PaneLayoutRect
  public var contentSize: PaneLayoutSize
  public var overflowAxes: Set<PaneLayoutAxis>
  public var surface: WritingSurfaceLayout

  public init(
    headerRect: PaneLayoutRect, toolbarRect: PaneLayoutRect?,
    reviewRect: PaneLayoutRect?, contentRect: PaneLayoutRect,
    contentSize: PaneLayoutSize, overflowAxes: Set<PaneLayoutAxis>,
    surface: WritingSurfaceLayout
  ) {
    self.headerRect = headerRect
    self.toolbarRect = toolbarRect
    self.reviewRect = reviewRect
    self.contentRect = contentRect
    self.contentSize = contentSize
    self.overflowAxes = overflowAxes
    self.surface = surface
  }
}

public struct WritingLayoutRequest: Equatable, Sendable {
  public var kind: PaneLayoutKind
  public var viewport: PaneLayoutSize
  public var font: PaneLayoutFontMetric
  public var fullWidth: Bool
  public var reviewVisible: Bool
  public var smartTypeVisible: Bool

  public init(
    kind: PaneLayoutKind, viewport: PaneLayoutSize, font: PaneLayoutFontMetric,
    fullWidth: Bool = false, reviewVisible: Bool = false,
    smartTypeVisible: Bool = true
  ) {
    self.kind = kind
    self.viewport = viewport
    self.font = font
    self.fullWidth = fullWidth
    self.reviewVisible = reviewVisible
    self.smartTypeVisible = smartTypeVisible
  }
}

public struct WritingCompanionRequest: Equatable, Sendable {
  public var companion: PaneLayoutCompanion
  /// The pair's two direct leaves in canonical tree order.
  public var canonicalPaneIds: [String]
  public var writingKind: PaneLayoutKind
  public var writingFont: PaneLayoutFontMetric
  public var viewport: PaneLayoutSize
  public var selectedPaneId: String?

  public init(
    companion: PaneLayoutCompanion, canonicalPaneIds: [String],
    writingKind: PaneLayoutKind, writingFont: PaneLayoutFontMetric,
    viewport: PaneLayoutSize,
    selectedPaneId: String? = nil
  ) {
    self.companion = companion
    self.canonicalPaneIds = canonicalPaneIds
    self.writingKind = writingKind
    self.writingFont = writingFont
    self.viewport = viewport
    self.selectedPaneId = selectedPaneId
  }
}

public struct WritingCompanionLayoutPlan: Equatable, Sendable {
  public var presentation: WritingCompanionPresentation
  /// The space owned by the pair. In side/below modes, persisted tree weights divide this rect.
  public var contentRect: PaneLayoutRect
  public var tabStripRect: PaneLayoutRect?
  public var tabOrder: [String]
  public var selectedPaneId: String
  public var contentSize: PaneLayoutSize
  public var overflowAxes: Set<PaneLayoutAxis>

  public init(
    presentation: WritingCompanionPresentation,
    contentRect: PaneLayoutRect, tabStripRect: PaneLayoutRect?,
    tabOrder: [String], selectedPaneId: String, contentSize: PaneLayoutSize,
    overflowAxes: Set<PaneLayoutAxis>
  ) {
    self.presentation = presentation
    self.contentRect = contentRect
    self.tabStripRect = tabStripRect
    self.tabOrder = tabOrder
    self.selectedPaneId = selectedPaneId
    self.contentSize = contentSize
    self.overflowAxes = overflowAxes
  }
}

public enum WritingLayoutError: Error, Equatable, Sendable {
  case unsupportedKind(PaneLayoutKind)
  case invalidFontMetric
  case fullWidthRequiresText
  case missingFloor(PaneLayoutKind)
  case invalidCompanionChildren
}

/// Pure geometry for writing panes. Platform views measure once at their root and consume this plan.
public struct WritingLayoutPolicy: Equatable, Sendable {
  public var metrics: WritingLayoutMetrics
  public var floors: [PaneLayoutKind: PaneLayoutSize]
  public var gaps: PaneLayoutGaps

  public init(
    metrics: WritingLayoutMetrics = .standard,
    floors: [PaneLayoutKind: PaneLayoutSize], gaps: PaneLayoutGaps
  ) {
    self.metrics = metrics
    self.floors = floors
    self.gaps = gaps
  }

  public func layout(_ request: WritingLayoutRequest) throws -> WritingLayoutPlan {
    try validate(request.font)
    if request.fullWidth && request.kind != .text {
      throw WritingLayoutError.fullWidthRequiresText
    }
    guard let measuredFloor = metrics.surfaceFloor(kind: request.kind, font: request.font) else {
      throw WritingLayoutError.unsupportedKind(request.kind)
    }
    let floor = combinedFloor(measuredFloor, floors[request.kind])

    let viewport = nonnegative(request.viewport)
    let contentSize = PaneLayoutSize(
      width: max(viewport.width, floor.width), height: max(viewport.height, floor.height))
    let overflow = overflowAxes(content: contentSize, viewport: viewport)

    switch request.kind {
    case .text:
      return textLayout(request, contentSize: contentSize, overflowAxes: overflow)
    case .screenplay:
      return screenplayLayout(request, contentSize: contentSize, overflowAxes: overflow)
    case .presentation:
      return presentationLayout(request, contentSize: contentSize, overflowAxes: overflow)
    default:
      throw WritingLayoutError.unsupportedKind(request.kind)
    }
  }

  public func companionLayout(
    _ request: WritingCompanionRequest
  ) throws -> WritingCompanionLayoutPlan {
    let companion = request.companion
    let expected = Set([companion.writingPaneId, companion.agentPaneId])
    guard request.canonicalPaneIds.count == 2,
      Set(request.canonicalPaneIds) == expected,
      request.canonicalPaneIds[0] != request.canonicalPaneIds[1]
    else { throw WritingLayoutError.invalidCompanionChildren }
    guard [.text, .screenplay, .presentation].contains(request.writingKind) else {
      throw WritingLayoutError.unsupportedKind(request.writingKind)
    }
    try validate(request.writingFont)
    guard
      let measuredWritingFloor = metrics.surfaceFloor(
        kind: request.writingKind, font: request.writingFont)
    else { throw WritingLayoutError.unsupportedKind(request.writingKind) }
    let writingFloor = combinedFloor(measuredWritingFloor, floors[request.writingKind])
    guard let agentFloor = floors[.agent] else {
      throw WritingLayoutError.missingFloor(.agent)
    }

    let viewport = nonnegative(request.viewport)
    let sideWidth = writingFloor.width + gaps.horizontal + agentFloor.width
    let belowHeight = writingFloor.height + gaps.vertical + agentFloor.height
    let selected =
      request.selectedPaneId.flatMap { expected.contains($0) ? $0 : nil }
      ?? request.canonicalPaneIds[0]

    if viewport.width >= sideWidth {
      let contentSize = PaneLayoutSize(
        width: viewport.width,
        height: max(viewport.height, max(writingFloor.height, agentFloor.height)))
      return companionPlan(
        presentation: .sideBySide,
        content: rect(x: 0, y: 0, width: contentSize.width, height: contentSize.height),
        tabStrip: nil,
        order: request.canonicalPaneIds, selected: selected,
        contentSize: contentSize, viewport: viewport)
    }

    if viewport.height >= belowHeight {
      let contentSize = PaneLayoutSize(
        width: max(viewport.width, max(writingFloor.width, agentFloor.width)),
        height: viewport.height)
      return companionPlan(
        presentation: .below,
        content: rect(x: 0, y: 0, width: contentSize.width, height: contentSize.height),
        tabStrip: nil,
        order: request.canonicalPaneIds, selected: selected,
        contentSize: contentSize, viewport: viewport)
    }

    let paneHeight = max(writingFloor.height, agentFloor.height)
    let contentSize = PaneLayoutSize(
      width: max(viewport.width, max(writingFloor.width, agentFloor.width)),
      height: max(viewport.height, metrics.companionTabStripHeight + paneHeight))
    let paneRect = rect(
      x: 0, y: metrics.companionTabStripHeight,
      width: contentSize.width, height: contentSize.height - metrics.companionTabStripHeight)
    return companionPlan(
      presentation: .tabs,
      content: paneRect,
      tabStrip: rect(
        x: 0, y: 0, width: contentSize.width, height: metrics.companionTabStripHeight),
      order: request.canonicalPaneIds, selected: selected,
      contentSize: contentSize, viewport: viewport)
  }

  private func textLayout(
    _ request: WritingLayoutRequest, contentSize: PaneLayoutSize,
    overflowAxes: Set<PaneLayoutAxis>
  ) -> WritingLayoutPlan {
    let inset = metrics.paneInset
    let header = rect(
      x: inset, y: inset, width: contentSize.width - 2 * inset,
      height: metrics.headerHeight)
    let toolbarY = header.origin.y + header.size.height + metrics.stackSpacing
    let toolbar = rect(
      x: inset, y: toolbarY, width: contentSize.width - 2 * inset,
      height: metrics.textToolbarHeight)
    var nextY = toolbar.origin.y + toolbar.size.height + metrics.stackSpacing
    var review: PaneLayoutRect?
    if request.reviewVisible {
      review = rect(
        x: inset, y: nextY, width: contentSize.width - 2 * inset,
        height: metrics.textReviewHeight)
      nextY += metrics.textReviewHeight + metrics.stackSpacing
    }
    let body = rect(
      x: inset, y: nextY, width: contentSize.width - 2 * inset,
      height: contentSize.height - nextY - inset)
    let available = max(0, body.size.width - 2 * metrics.textPageInset)
    let minimum = metrics.textCompactCharacters * request.font.characterWidth
    let ideal = metrics.textReadingCharacters * request.font.characterWidth
    let width = request.fullWidth ? max(minimum, available) : max(minimum, min(ideal, available))
    let column = rect(
      x: body.origin.x + metrics.textPageInset + max(0, (available - width) / 2),
      y: body.origin.y + metrics.textPageInset,
      width: width, height: body.size.height - 2 * metrics.textPageInset)
    return WritingLayoutPlan(
      headerRect: header, toolbarRect: toolbar, reviewRect: review,
      contentRect: body, contentSize: contentSize, overflowAxes: overflowAxes,
      surface: .text(
        WritingTextLayoutPlan(
          mode: request.fullWidth ? .fullWidth : .reading,
          columnRect: column, visibleCharacters: width / request.font.characterWidth)))
  }

  private func screenplayLayout(
    _ request: WritingLayoutRequest, contentSize: PaneLayoutSize,
    overflowAxes: Set<PaneLayoutAxis>
  ) -> WritingLayoutPlan {
    let header = rect(x: 0, y: 0, width: contentSize.width, height: metrics.headerHeight)
    let smartTypeHeight = request.smartTypeVisible ? metrics.screenplaySmartTypeHeight : 0
    let toolbar = rect(
      x: 0, y: metrics.headerHeight, width: contentSize.width,
      height: metrics.screenplayToolbarHeight + smartTypeHeight)
    let bodyY = toolbar.origin.y + toolbar.size.height
    let body = rect(
      x: 0, y: bodyY, width: contentSize.width, height: contentSize.height - bodyY)
    let pageWidth =
      metrics.screenplayColumns * request.font.characterWidth
      + 2 * metrics.screenplayPageHorizontalInset
    let pageSize = PaneLayoutSize(width: pageWidth, height: pageWidth * metrics.screenplayPageRatio)
    let showsRail =
      body.size.width
      >= metrics.screenplaySceneRailWidth + metrics.railDividerWidth + pageSize.width
    let navigation: WritingSceneNavigationMode = showsRail ? .rail : .drawer
    let navigationRect =
      showsRail
      ? rect(
        x: body.origin.x, y: body.origin.y,
        width: metrics.screenplaySceneRailWidth, height: body.size.height)
      : nil
    let pageArea = rect(
      x: body.origin.x
        + (showsRail ? metrics.screenplaySceneRailWidth + metrics.railDividerWidth : 0),
      y: body.origin.y,
      width: body.size.width
        - (showsRail ? metrics.screenplaySceneRailWidth + metrics.railDividerWidth : 0),
      height: body.size.height)
    let fittedScale = min(
      1, min(pageArea.size.width / pageSize.width, pageArea.size.height / pageSize.height))
    let scale = max(metrics.screenplayMinimumScale, fittedScale)
    let scaled = PaneLayoutSize(width: pageSize.width * scale, height: pageSize.height * scale)
    let page = centered(scaled, in: pageArea)
    var combinedOverflow = overflowAxes
    if scaled.width > pageArea.size.width { combinedOverflow.insert(.horizontal) }
    if scaled.height > pageArea.size.height { combinedOverflow.insert(.vertical) }
    return WritingLayoutPlan(
      headerRect: header, toolbarRect: toolbar, reviewRect: nil,
      contentRect: body, contentSize: contentSize, overflowAxes: combinedOverflow,
      surface: .screenplay(
        WritingScreenplayLayoutPlan(
          navigation: navigation, navigationRect: navigationRect,
          pageSize: pageSize, pageRect: page, scale: scale)))
  }

  private func presentationLayout(
    _ request: WritingLayoutRequest, contentSize: PaneLayoutSize,
    overflowAxes: Set<PaneLayoutAxis>
  ) -> WritingLayoutPlan {
    let header = rect(x: 0, y: 0, width: contentSize.width, height: metrics.headerHeight)
    let footer = metrics.presentationFooterHeight
    let body = rect(
      x: 0, y: metrics.headerHeight, width: contentSize.width,
      height: contentSize.height - metrics.headerHeight - footer)
    let thumbnailThreshold =
      metrics.presentationThumbnailRailWidth + metrics.railDividerWidth
      + 2 * metrics.paneInset + metrics.presentationMinimumStage.width
    let showsThumbnails = body.size.width >= thumbnailThreshold
    let navigation: WritingSlideNavigationMode = showsThumbnails ? .thumbnails : .numbers
    let navigationRect: PaneLayoutRect
    let stageArea: PaneLayoutRect
    if showsThumbnails {
      navigationRect = rect(
        x: body.origin.x, y: body.origin.y,
        width: metrics.presentationThumbnailRailWidth, height: body.size.height)
      stageArea = rect(
        x: body.origin.x + metrics.presentationThumbnailRailWidth + metrics.railDividerWidth
          + metrics.paneInset,
        y: body.origin.y + metrics.paneInset,
        width: body.size.width - metrics.presentationThumbnailRailWidth
          - metrics.railDividerWidth - 2 * metrics.paneInset,
        height: body.size.height - 2 * metrics.paneInset)
    } else {
      navigationRect = rect(
        x: body.origin.x, y: body.origin.y, width: body.size.width,
        height: metrics.presentationNumericRailHeight)
      stageArea = rect(
        x: body.origin.x + metrics.paneInset,
        y: body.origin.y + metrics.presentationNumericRailHeight + metrics.paneInset,
        width: body.size.width - 2 * metrics.paneInset,
        height: body.size.height - metrics.presentationNumericRailHeight
          - 2 * metrics.paneInset)
    }
    let fitted = aspectFit(
      metrics.presentationMaximumStage, in: stageArea,
      minimum: metrics.presentationMinimumStage)
    var combinedOverflow = overflowAxes
    if fitted.width > stageArea.size.width { combinedOverflow.insert(.horizontal) }
    if fitted.height > stageArea.size.height { combinedOverflow.insert(.vertical) }
    return WritingLayoutPlan(
      headerRect: header, toolbarRect: nil, reviewRect: nil,
      contentRect: body, contentSize: contentSize, overflowAxes: combinedOverflow,
      surface: .presentation(
        WritingPresentationLayoutPlan(
          navigation: navigation, navigationRect: navigationRect,
          stageRect: centered(fitted, in: stageArea),
          footerRect: rect(
            x: 0, y: contentSize.height - footer,
            width: contentSize.width, height: footer),
          scale: fitted.width / metrics.presentationMinimumStage.width)))
  }

  private func companionPlan(
    presentation: WritingCompanionPresentation,
    content: PaneLayoutRect, tabStrip: PaneLayoutRect?,
    order: [String], selected: String, contentSize: PaneLayoutSize,
    viewport: PaneLayoutSize
  ) -> WritingCompanionLayoutPlan {
    WritingCompanionLayoutPlan(
      presentation: presentation, contentRect: content, tabStripRect: tabStrip,
      tabOrder: order, selectedPaneId: selected, contentSize: contentSize,
      overflowAxes: overflowAxes(content: contentSize, viewport: viewport))
  }

  private func validate(_ font: PaneLayoutFontMetric) throws {
    guard font.characterWidth.isFinite, font.characterWidth > 0,
      font.lineHeight.isFinite, font.lineHeight > 0,
      font.pointSize.isFinite, font.pointSize > 0
    else { throw WritingLayoutError.invalidFontMetric }
  }

  private func combinedFloor(
    _ measured: PaneLayoutSize, _ supplied: PaneLayoutSize?
  ) -> PaneLayoutSize {
    guard let supplied else { return measured }
    return PaneLayoutSize(
      width: max(measured.width, supplied.width),
      height: max(measured.height, supplied.height))
  }

  private func aspectFit(
    _ maximum: PaneLayoutSize, in area: PaneLayoutRect, minimum: PaneLayoutSize
  ) -> PaneLayoutSize {
    let availableScale = min(
      area.size.width / maximum.width, area.size.height / maximum.height)
    let minimumScale = max(
      minimum.width / maximum.width, minimum.height / maximum.height)
    let scale = max(minimumScale, min(1, availableScale))
    return PaneLayoutSize(width: maximum.width * scale, height: maximum.height * scale)
  }

  private func centered(
    _ size: PaneLayoutSize, in area: PaneLayoutRect
  ) -> PaneLayoutRect {
    rect(
      x: area.origin.x + max(0, (area.size.width - size.width) / 2),
      y: area.origin.y + max(0, (area.size.height - size.height) / 2),
      width: size.width, height: size.height)
  }

  private func rect(
    x: Double, y: Double, width: Double, height: Double
  ) -> PaneLayoutRect {
    PaneLayoutRect(
      origin: PaneLayoutPoint(x: x, y: y),
      size: PaneLayoutSize(width: max(0, width), height: max(0, height)))
  }

  private func nonnegative(_ size: PaneLayoutSize) -> PaneLayoutSize {
    PaneLayoutSize(width: max(0, size.width), height: max(0, size.height))
  }

  private func overflowAxes(
    content: PaneLayoutSize, viewport: PaneLayoutSize
  ) -> Set<PaneLayoutAxis> {
    var axes: Set<PaneLayoutAxis> = []
    if content.width > viewport.width { axes.insert(.horizontal) }
    if content.height > viewport.height { axes.insert(.vertical) }
    return axes
  }
}
