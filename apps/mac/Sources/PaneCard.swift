import SoyehtClient
import SoyehtProtocol
import SwiftUI

private struct AgentPaneHydrationTaskKey: Equatable {
  let connectionRevision: UInt64
  let isVisible: Bool
}

/// One pane of the grid: the editor, a writing pane, or a header over the pane's body, with the
/// drag-and-drop target, the context menu and the loop panel around it.
struct PaneCard: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let pane: Pane
  let tint: Color
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let interaction: PaneLayoutInteractionState?
  let companionLinked: Bool
  let isVisible: Bool
  let cardWidth: CGFloat
  let dropTargetSize: CGSize?
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void
  @State private var showingAutomation = false
  @State private var capture = PaneExperimentCapture()

  var body: some View {
    Group {
      if pane.mode == .editor {
        EditorPaneView(
          client: client, pane: pane, headerDrag: dragConfiguration,
          projectPanelFits: MacPaneLayoutMetrics.editorShowsProjectPanel(cardWidth: cardWidth),
          isVisible: isVisible)
      } else if pane.mode == .writing, pane.writingKind == .text {
        WritingTextPane(
          client: client, pane: pane, headerDrag: dragConfiguration,
          companionLinked: companionLinked, onSplit: onSplit,
          layoutActionsDisabled: interaction?.blocksLayoutActions == true)
      } else if pane.mode == .writing, pane.writingKind == .presentation {
        WritingPresentationPane(
          client: client, pane: pane, headerDrag: dragConfiguration,
          companionLinked: companionLinked, onSplit: onSplit,
          layoutActionsDisabled: interaction?.blocksLayoutActions == true)
      } else if pane.mode == .writing, pane.writingKind == .screenplay {
        WritingScreenplayPane(
          client: client, pane: pane, headerDrag: dragConfiguration,
          companionLinked: companionLinked, onSplit: onSplit,
          layoutActionsDisabled: interaction?.blocksLayoutActions == true)
      } else {
        VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
          PaneCardHeader(
            client: client, pane: pane, cardWidth: cardWidth, companionLinked: companionLinked,
            interaction: interaction, dragConfiguration: dragConfiguration, capture: capture,
            showingAutomation: $showingAutomation,
            onTasks: onTasks, onSplit: onSplit, onClose: onClose)
          PaneCardBody(client: client, webHost: webHost, pane: pane, isVisible: isVisible)
        }
        .padding(PaneLayoutContentMetrics.paneCardInset)
        .paneShell()
      }
    }
    .contentShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    .task(
      id: AgentPaneHydrationTaskKey(
        connectionRevision: client.connectionRevision, isVisible: isVisible
      )
    ) {
      guard pane.mode == .agent else { return }
      await client.hydratePresentedAgentPanes([
        PresentedPane(paneId: pane.paneId, isVisible: isVisible)
      ])
    }
    .simultaneousGesture(TapGesture().onEnded { client.select(pane.paneId) })
    .modifier(
      PaneDropTargetModifier(
        interaction: interaction, workspaceId: pane.workspaceId, paneId: pane.paneId,
        size: dropTargetSize, onPaneDrop: onPaneDrop)
    )
    .overlay(alignment: .topLeading) {
      if let interaction, let dropTargetSize {
        PaneDropZonesOverlay(
          paneId: pane.paneId, size: dropTargetSize,
          selected: interaction.dropTarget?.paneId == pane.paneId
            ? interaction.dropTarget?.zone : nil)
      }
    }
    .contextMenu { Button("Close pane", role: .destructive) { onClose(pane.paneId) } }
    .experimentCaptureDialogs(capture, pane: pane, client: client)
    .overlay(alignment: .topTrailing) {
      if showingAutomation {
        PaneAutomationPanel(client: client, pane: pane) { showingAutomation = false }
          .offset(x: -6, y: 38)
          .transition(.scale(scale: 0.98, anchor: .topTrailing).combined(with: .opacity))
          .zIndex(20)
      }
    }
    .zIndex(showingAutomation ? 20 : 0)
  }

  private var dragConfiguration: PaneHeaderDragConfiguration? {
    guard let interaction else { return nil }
    return PaneHeaderDragConfiguration(
      interaction: interaction, workspaceId: pane.workspaceId, paneId: pane.paneId,
      title: pane.name, tint: tint)
  }
}
