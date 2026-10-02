import AppKit
import SoyehtClient
import SwiftUI

/// The hit area between two sibling panes: drag to resize, double-click to equalize.
struct PaneLayoutDividerHandle: View {
  let axis: PaneLayoutAxis
  let accessibilityID: String
  let onChanged: (CGSize) -> Void
  let onEnded: () -> Void
  let onDoubleClick: () -> Void
  @State private var cursorIsActive = false

  var body: some View {
    Color.clear
      .contentShape(Rectangle())
      .gesture(
        DragGesture(minimumDistance: 2)
          .onChanged { onChanged($0.translation) }
          .onEnded { _ in onEnded() }
      )
      .onTapGesture(count: 2, perform: onDoubleClick)
      .onHover { inside in
        guard inside != cursorIsActive else { return }
        cursorIsActive = inside
        if inside {
          resizeCursor.push()
        } else {
          NSCursor.pop()
        }
      }
      .onDisappear {
        if cursorIsActive {
          NSCursor.pop()
          cursorIsActive = false
        }
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(axis == .horizontal ? "Resize columns" : "Resize rows")
      .accessibilityIdentifier(accessibilityID)
  }

  private var resizeCursor: NSCursor {
    axis == .horizontal ? .resizeLeftRight : .resizeUpDown
  }
}
