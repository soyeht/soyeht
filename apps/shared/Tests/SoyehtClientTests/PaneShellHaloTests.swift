#if os(macOS)
  import AppKit
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// The pane shell's halo must reach past every edge of every pane by the design's blur
  /// radius, between panes and at the window edge, and fade out before the window ends: the
  /// approved frames draw the whole halo, so nothing may clip it.
  @Test func windowMarginsAndGapsLeaveRoomForTheWholeHalo() {
    #expect(PaneLayoutSpacing.outerMargin >= Double(PaneShellMetrics.halo))
    #expect(PaneLayoutSpacing.paneGap >= Double(PaneShellMetrics.shadowBlur))
    #expect(PaneShellMetrics.halo == 15)
  }

  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoMidnightTeal])
  func paneShellHaloIsCompleteOnEveryEdge(theme: TerminalColorTheme) throws {
    for scenario in HaloScenario.allCases {
      let render = try #require(HaloRender(theme: theme, scenario: scenario, scrolls: false))
      for rect in render.panes {
        for side in HaloRender.Side.allCases {
          #expect(
            render.haloReaches(rect, side, by: PaneShellMetrics.shadowBlur),
            "\(theme) \(scenario) \(side): halo cut before the blur radius")
        }
      }
      #expect(render.windowEdgesAreCanvas, "\(theme) \(scenario): halo runs into the window edge")
      if scenario == .singleAtRightEdge, let pane = render.panes.first {
        // The probe reads the halo where the design draws it, and bare canvas beyond it.
        for side in HaloRender.Side.allCases {
          #expect(!render.haloReaches(pane, side, by: PaneShellMetrics.halo + 4), "\(side)")
        }
      }
    }
  }

  /// The same panes inside a scroll view lose the halo at the scroll view's edge: the reason
  /// `PaneLayoutContainer` only scrolls a split that overflows.
  @MainActor @Test func aScrollViewWouldClipTheHalo() throws {
    let render = try #require(
      HaloRender(theme: .neoMilk, scenario: .singleAtRightEdge, scrolls: true))
    let pane = try #require(render.panes.first)
    #expect(!render.haloReaches(pane, .trailing, by: PaneShellMetrics.shadowBlur))
  }

  enum HaloScenario: CaseIterable {
    case sideBySide, singleAtRightEdge, stacked
  }

  @MainActor struct HaloRender {
    enum Side: CaseIterable { case leading, trailing, top, bottom }

    static let window = CGSize(width: 900, height: 560)
    let panes: [CGRect]
    private let bitmap: NSBitmapImageRep
    private let canvas: (Int, Int, Int)

    init?(theme: TerminalColorTheme, scenario: HaloScenario, scrolls: Bool) {
      let palette = theme.palette
      let colors = SoyehtThemeColorSet(palette: palette)
      let margin = PaneLayoutSpacing.outerMargin
      let viewport = PaneLayoutSize(
        width: Self.window.width - 2 * margin, height: Self.window.height - 2 * margin)
      let leaves = (0..<(scenario == .singleAtRightEdge ? 1 : 2)).map {
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "pane-\($0)", kind: .agent)))
      }
      let tree: PaneLayoutTree =
        leaves.count == 1
        ? leaves[0].tree
        : .split(
          PaneLayoutSplit(axis: scenario == .stacked ? .vertical : .horizontal, children: leaves))
      let floor = PaneLayoutSize(width: 200, height: 160)
      let sizing = PaneLayoutSizingPolicy(
        floors: Dictionary(uniqueKeysWithValues: PaneLayoutKind.allCases.map { ($0, floor) }),
        gaps: PaneLayoutSpacing.gaps)
      guard
        let plan = try? PaneLayoutGeometry.layout(tree: tree, viewport: viewport, sizing: sizing)
      else { return nil }
      let rects = plan.paneRects.values.map {
        CGRect(x: $0.origin.x, y: $0.origin.y, width: $0.size.width, height: $0.size.height)
      }
      let size = CGSize(width: viewport.width, height: viewport.height)
      let content = PaneLayoutContainer(
        overflowAxes: scrolls ? .vertical : [], size: size, contentSize: size
      ) {
        ZStack(alignment: .topLeading) {
          ForEach(Array(rects.enumerated()), id: \.offset) { _, rect in
            Color.clear.paneShell(colors)
              .frame(width: rect.width, height: rect.height)
              .offset(x: rect.minX, y: rect.minY)
          }
        }
      }
      .padding(margin)
      .background(colors.canvas)
      guard let bitmap = PaneInteriorRendering.bitmap(content, size: Self.window),
        let canvas = ThemeHexColorMath.rgb8(palette.canvas)
      else { return nil }
      self.bitmap = bitmap
      self.canvas = (Int(canvas.0), Int(canvas.1), Int(canvas.2))
      panes = rects.map { $0.offsetBy(dx: margin, dy: margin) }
    }

    /// True when the pixel `distance` points outside the middle of `side` is not bare canvas.
    func haloReaches(_ rect: CGRect, _ side: Side, by distance: CGFloat) -> Bool {
      let point =
        switch side {
        case .leading: CGPoint(x: rect.minX - distance, y: rect.midY)
        case .trailing: CGPoint(x: rect.maxX + distance, y: rect.midY)
        case .top: CGPoint(x: rect.midX, y: rect.minY - distance)
        case .bottom: CGPoint(x: rect.midX, y: rect.maxY + distance)
        }
      return !isCanvas(point)
    }

    /// The outermost point column and row of the window are untouched canvas.
    var windowEdgesAreCanvas: Bool {
      let edge = CGFloat(0.5)
      let size = Self.window
      return stride(from: CGFloat(2), to: size.height - 2, by: 8).allSatisfy {
        isCanvas(CGPoint(x: edge, y: $0)) && isCanvas(CGPoint(x: size.width - edge, y: $0))
      }
        && stride(from: CGFloat(2), to: size.width - 2, by: 8).allSatisfy {
          isCanvas(CGPoint(x: $0, y: edge)) && isCanvas(CGPoint(x: $0, y: size.height - edge))
        }
    }

    private func isCanvas(_ point: CGPoint) -> Bool {
      let x = min(bitmap.pixelsWide - 1, max(0, Int(point.x * 2)))
      let y = min(bitmap.pixelsHigh - 1, max(0, Int(point.y * 2)))
      guard let data = bitmap.bitmapData else { return false }
      let pixel = data + y * bitmap.bytesPerRow + x * (bitmap.bitsPerPixel / 8)
      return Int(pixel[0]) == canvas.0 && Int(pixel[1]) == canvas.1 && Int(pixel[2]) == canvas.2
    }
  }
#endif
