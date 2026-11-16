import AppKit
import SoyehtClient
import SwiftUI

/// The avatar of a workspace on the Mac: the shared stage on a flat surface of the palette. It
/// only draws what the model gives it; the state of the conversation is the model's to set. The
/// stage stops while its window is covered or minimised, and a package that did not load says so
/// here, by its kind, in the palette's colours.
struct MacAvatarStage: View {
  let model: AvatarStageModel
  @State private var covered = false

  var body: some View {
    ZStack {
      AvatarStageView(model: model, visible: !covered)
      if let text = model.failureText {
        Text(verbatim: text)
          .font(MacTheme.ui(MacTheme.TypeScale.control))
          .foregroundStyle(MacTheme.secondary)
          .multilineTextAlignment(.center)
          .padding(MacTheme.Space.x8)
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(MacTheme.canvas)
    .background(WindowCover(covered: $covered))
    .accessibilityIdentifier("avatar-stage")
  }
}

/// Reports whether the window the view is in is covered by others or minimised.
private struct WindowCover: NSViewRepresentable {
  @Binding var covered: Bool

  func makeNSView(context: Context) -> CoverView {
    let view = CoverView()
    view.onChange = { covered = $0 }
    return view
  }

  func updateNSView(_ view: CoverView, context: Context) {
    view.onChange = { covered = $0 }
  }

  final class CoverView: NSView {
    var onChange: ((Bool) -> Void)?
    private var watching: [Task<Void, Never>] = []

    override func viewDidMoveToWindow() {
      for task in watching { task.cancel() }
      watching = []
      guard let window else { return }
      report(window)
      for name in [
        NSWindow.didChangeOcclusionStateNotification, NSWindow.didMiniaturizeNotification,
        NSWindow.didDeminiaturizeNotification,
      ] {
        watching.append(
          Task { @MainActor [weak self, weak window] in
            for await _ in NotificationCenter.default.notifications(named: name, object: window) {
              guard let window else { return }
              self?.report(window)
            }
          })
      }
    }

    private func report(_ window: NSWindow) {
      onChange?(window.isMiniaturized || !window.occlusionState.contains(.visible))
    }
  }
}
