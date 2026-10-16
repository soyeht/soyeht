import SoyehtClient
import SwiftUI
import UniformTypeIdentifiers

struct PaneDragSource: Equatable {
  let workspaceId: String
  let paneId: String
}

struct PaneDividerGesture: Equatable {
  let workspaceId: String
  let splitPath: PaneLayoutNodePath
  let afterChildIndex: Int
  let baseTree: PaneLayoutTree
}

/// What a pane or divider drag is doing right now: its source, the drop target under the pointer,
/// and the divider being resized.
@Observable
final class PaneLayoutInteractionState {
  var paneSource: PaneDragSource?
  var dropTarget: PaneLayoutDropTarget?
  var divider: PaneDividerGesture?

  var blocksLayoutActions: Bool { divider != nil }

  func beginPaneDrag(workspaceId: String, paneId: String) {
    divider = nil
    dropTarget = nil
    paneSource = PaneDragSource(workspaceId: workspaceId, paneId: paneId)
  }

  func updateDropTarget(_ target: PaneLayoutDropTarget?) {
    dropTarget = target
  }

  func beginDivider(_ gesture: PaneDividerGesture) {
    paneSource = nil
    dropTarget = nil
    divider = gesture
  }

  func clearDropTarget(for paneId: String) {
    if dropTarget?.paneId == paneId { dropTarget = nil }
  }

  func clear() {
    paneSource = nil
    dropTarget = nil
    divider = nil
  }
}

struct PaneHeaderDragConfiguration {
  let interaction: PaneLayoutInteractionState
  let workspaceId: String
  let paneId: String
  let title: String
  let tint: Color
}

enum PaneDragTransfer {
  static let type = UTType(exportedAs: PaneDragTransferPolicy.typeIdentifier, conformingTo: .data)
  static let acceptedTypes = [type]

  static func accepts(_ info: DropInfo) -> Bool {
    PaneDragTransferPolicy.accepts(
      info.itemProviders(for: acceptedTypes).flatMap(\.registeredTypeIdentifiers))
  }

  static func itemProvider() -> NSItemProvider {
    let provider = NSItemProvider()
    provider.registerDataRepresentation(
      forTypeIdentifier: type.identifier, visibility: .ownProcess
    ) {
      completion in
      completion(Data(), nil)
      return nil
    }
    return provider
  }
}

extension View {
  @ViewBuilder
  func paneHeaderDrag(_ configuration: PaneHeaderDragConfiguration?) -> some View {
    if let configuration {
      onDrag {
        configuration.interaction.beginPaneDrag(
          workspaceId: configuration.workspaceId, paneId: configuration.paneId)
        return PaneDragTransfer.itemProvider()
      } preview: {
        PaneDragGhost(title: configuration.title, tint: configuration.tint)
      }
    } else {
      self
    }
  }
}

private struct PaneDragGhost: View {
  let title: String
  let tint: Color

  var body: some View {
    HStack(spacing: MacTheme.Space.x7) {
      Image(systemName: "rectangle.split.2x1")
      Text(title).lineLimit(1)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
    .foregroundStyle(MacTheme.ink)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.genericPaneHeaderHeight)
    .background(tint.opacity(0.92), in: Capsule())
    .shadow(color: MacTheme.ink.opacity(0.16), radius: 8, y: 4)
  }
}

struct PaneLayoutPaneDropDelegate: DropDelegate {
  let interaction: PaneLayoutInteractionState
  let workspaceId: String
  let targetPaneId: String
  let targetSize: CGSize
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  func validateDrop(info: DropInfo) -> Bool {
    guard let source = interaction.paneSource, PaneDragTransfer.accepts(info) else { return false }
    return source.workspaceId == workspaceId && source.paneId != targetPaneId
  }

  func dropEntered(info: DropInfo) {
    updateTarget(info.location)
  }

  func dropUpdated(info: DropInfo) -> DropProposal? {
    updateTarget(info.location)
    return interaction.dropTarget == nil
      ? DropProposal(operation: .cancel)
      : DropProposal(
        operation: .move)
  }

  func dropExited(info: DropInfo) {
    interaction.clearDropTarget(for: targetPaneId)
  }

  func performDrop(info: DropInfo) -> Bool {
    updateTarget(info.location)
    guard let source = interaction.paneSource,
      let target = interaction.dropTarget,
      target.paneId == targetPaneId,
      target.workspaceId == workspaceId
    else {
      interaction.clear()
      return false
    }
    onPaneDrop(source.paneId, target.paneId, target.zone)
    interaction.clear()
    return true
  }

  private func updateTarget(_ location: CGPoint) {
    guard let source = interaction.paneSource else {
      interaction.updateDropTarget(nil)
      return
    }
    let rect = PaneLayoutRect(
      size: PaneLayoutSize(width: targetSize.width, height: targetSize.height))
    interaction.updateDropTarget(
      PaneLayoutDropGeometry.target(
        at: PaneLayoutPoint(x: location.x, y: location.y),
        sourceWorkspaceId: source.workspaceId, targetWorkspaceId: workspaceId,
        targetPaneId: targetPaneId, paneArea: rect, targetPaneRect: rect))
  }
}

struct PaneLayoutFallbackDropDelegate: DropDelegate {
  let interaction: PaneLayoutInteractionState

  func validateDrop(info: DropInfo) -> Bool {
    interaction.paneSource != nil && PaneDragTransfer.accepts(info)
  }

  func performDrop(info: DropInfo) -> Bool {
    interaction.clear()
    return false
  }
}

struct PaneLayoutChooserDropDelegate: DropDelegate {
  let interaction: PaneLayoutInteractionState

  private var plan: PaneLayoutDropPresentation {
    PaneLayoutDropPresentationPolicy.plan(for: .chooser)
  }

  func validateDrop(info: DropInfo) -> Bool {
    plan.acceptsDrop && interaction.paneSource != nil && PaneDragTransfer.accepts(info)
  }

  func dropEntered(info: DropInfo) {
    interaction.updateDropTarget(nil)
  }

  func dropUpdated(info: DropInfo) -> DropProposal? {
    interaction.updateDropTarget(nil)
    return DropProposal(operation: .move)
  }

  func dropExited(info: DropInfo) {
    interaction.updateDropTarget(nil)
  }

  func performDrop(info: DropInfo) -> Bool {
    interaction.clear()
    return plan.completesAsNoOp
  }
}

struct PaneDropTargetModifier: ViewModifier {
  let interaction: PaneLayoutInteractionState?
  let workspaceId: String
  let paneId: String
  let size: CGSize?
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  @ViewBuilder
  func body(content: Content) -> some View {
    if let interaction, let size {
      content.onDrop(
        of: PaneDragTransfer.acceptedTypes,
        delegate: PaneLayoutPaneDropDelegate(
          interaction: interaction, workspaceId: workspaceId, targetPaneId: paneId,
          targetSize: size, onPaneDrop: onPaneDrop))
    } else {
      content
    }
  }
}

struct PaneDropZonesOverlay: View {
  let paneId: String
  let size: CGSize
  let selected: PaneLayoutDropZone?

  var body: some View {
    ZStack(alignment: .topLeading) {
      ForEach(PaneLayoutDropZone.allCases, id: \.rawValue) { zone in
        let rect = PaneLayoutDropGeometry.rect(
          for: zone,
          in: PaneLayoutRect(size: PaneLayoutSize(width: size.width, height: size.height)))
        RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
          .fill(selected == zone ? MacTheme.accent.opacity(0.24) : Color.clear)
          .overlay {
            if selected == zone {
              RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
                .stroke(MacTheme.accent, lineWidth: 2)
            }
          }
          .frame(width: rect.size.width, height: rect.size.height)
          .offset(x: rect.origin.x, y: rect.origin.y)
          .accessibilityElement(children: .ignore)
          .accessibilityLabel("Drop pane \(zone.rawValue)")
          .accessibilityIdentifier("pane-drop-zone-\(zone.rawValue)-\(paneId)")
      }
    }
    .frame(width: size.width, height: size.height, alignment: .topLeading)
    .allowsHitTesting(false)
  }
}
