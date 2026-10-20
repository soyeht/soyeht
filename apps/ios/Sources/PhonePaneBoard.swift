import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The floors the board arranges panes with. They come from the same formulas as the Mac's,
/// with the fonts this app draws, so `PaneLayoutFit` stacks and tabs the panes by the same
/// rules and the same vectors.
@MainActor enum PhonePaneBoardMetrics {
  static let sizing = PaneLayoutSizingPolicy(
    floors: PaneLayoutFloorFormula.footprintFloors(
      fonts: PhoneWritingMetrics.fonts, halo: PaneLayoutSpacing.paneHalo),
    gaps: PaneLayoutSpacing.gaps,
    compactFloors: PaneLayoutFloorFormula.compactFootprintFloors(
      fonts: PhoneWritingMetrics.fonts, halo: PaneLayoutSpacing.paneHalo))
}

/// A workspace's panes side by side on a wide screen, arranged from the layout the Mac stores
/// for the workspace. Panes that do not fit stack, then share a tab group; the person's layout
/// itself is never edited from here. Focus follows a tap, and a hardware keyboard moves it.
struct PhonePaneBoardView: View {
  let client: EngineClient
  let workspaceId: String
  @Binding var focusedPaneId: String?
  @State private var shownTab: String?

  var body: some View {
    GeometryReader { proxy in
      let viewport = PaneBoardPolicy.viewport(
        in: PaneLayoutSize(width: proxy.size.width, height: proxy.size.height))
      Group {
        if let board = client.paneBoard(
          workspaceId: workspaceId, viewport: viewport, sizing: PhonePaneBoardMetrics.sizing)
        {
          content(board, viewport: viewport)
        } else {
          ContentUnavailableView(
            "No open panes", systemImage: "rectangle.split.2x1",
            description: Text("Open a pane on the Mac to continue here."))
        }
      }
      .frame(width: proxy.size.width, height: proxy.size.height)
    }
    .background(PhonePalette.canvas)
    .overlay(alignment: .bottom) {
      if TerminalCopyNotice.shared.visible { PhoneCopyNotice() }
    }
    .navigationTitle(client.workspaces.first { $0.workspaceId == workspaceId }?.name ?? "")
    .navigationBarTitleDisplayMode(.inline)
  }

  private func content(_ board: PaneBoard, viewport: PaneLayoutSize) -> some View {
    let shown = board.shownTab(focused: focusedPaneId, remembered: shownTab)
    let colors = SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette)
    return PaneLayoutContainer(
      overflowAxes: Axis.Set(overflow: board.overflowAxes(in: viewport)),
      size: CGSize(width: viewport.width, height: viewport.height),
      contentSize: CGSize(width: board.contentSize.width, height: board.contentSize.height)
    ) {
      ZStack(alignment: .topLeading) {
        ForEach(board.placements, id: \.paneId) { placement in
          card(placement.paneId, colors: colors).writingFrame(placement.rect)
        }
        if let tabs = board.tabs, let shown {
          PhonePaneTabStrip(client: client, paneIds: tabs.paneIds, shown: shown) { select($0) }
            .writingFrame(tabs.strip)
          card(shown, colors: colors).id(shown).writingFrame(tabs.area)
        }
      }
    }
    .padding(PaneBoardPolicy.margin)
    .onChange(of: board, initial: true) { _, board in
      let focus = board.reconciledFocus(focusedPaneId, preferred: client.selectedPaneId)
      if focus != focusedPaneId { focusedPaneId = focus }
    }
    .onChange(of: shown) { if let shown { shownTab = shown } }
    .task(id: HydrationKey(revision: client.connectionRevision, paneIds: board.paneIds)) {
      await client.hydratePresentedAgentPanes(client.presentedAgents(in: board.paneIds))
    }
    .onChange(of: focusedPaneId, initial: true) { _, paneId in client.select(paneId) }
    .focusedSceneValue(\.phonePaneBoardNavigation, navigation(board, shown: shown))
  }

  @ViewBuilder private func card(_ paneId: String, colors: SoyehtThemeColorSet) -> some View {
    if let pane = client.panes.first(where: { $0.paneId == paneId }) {
      PhonePaneBoardCard(
        client: client, pane: pane, isFocused: focusedPaneId == paneId, colors: colors
      ) { select(paneId) }
    }
  }

  private func select(_ paneId: String) {
    if focusedPaneId != paneId { focusedPaneId = paneId }
  }

  private func navigation(_ board: PaneBoard, shown: String?) -> PhonePaneBoardNavigation {
    PhonePaneBoardNavigation(
      step: { offset in
        if let pane = board.focus(after: focusedPaneId, by: offset) { select(pane) }
      },
      move: { direction in
        if let pane = board.neighbor(of: focusedPaneId, toward: direction, shown: shown) {
          select(pane)
        }
      })
  }

  private struct HydrationKey: Equatable {
    let revision: UInt64
    let paneIds: [String]
  }
}

/// The tabs of the panes that did not fit side by side. Flat: the shown tab sits on the `face`.
private struct PhonePaneTabStrip: View {
  let client: EngineClient
  let paneIds: [String]
  let shown: String
  let select: (String) -> Void

  var body: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: PaneInteriorMetrics.compactSpacing) {
        ForEach(paneIds, id: \.self) { paneId in
          if let pane = client.panes.first(where: { $0.paneId == paneId }) {
            tab(pane)
          }
        }
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("pane-board-tabs")
  }

  private func tab(_ pane: Pane) -> some View {
    let isShown = pane.paneId == shown
    return Button {
      select(pane.paneId)
    } label: {
      HStack(spacing: PaneInteriorMetrics.tightSpacing) {
        PaneStateDot(pane.state)
        Text(pane.name).lineLimit(1)
      }
      .font(
        PaneInteriorFont.ui(PaneInteriorMetrics.bodyPointSize, weight: isShown ? .heavy : .medium)
      )
      .foregroundStyle(isShown ? PhonePalette.ink : PhonePalette.secondary)
      .padding(.horizontal, PaneInteriorMetrics.phoneInset)
      .padding(.vertical, PaneInteriorMetrics.tightSpacing)
      .background(
        isShown ? PhonePalette.face : Color.clear,
        in: RoundedRectangle(cornerRadius: PaneInteriorMetrics.cardRadius, style: .continuous))
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(isShown ? .isSelected : [])
  }
}
