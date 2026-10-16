import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The experiment recording of one pane: whether a request is in flight, what the person was
/// last told, and the two questions the recording asks (start it, keep or delete its data).
@MainActor @Observable final class PaneExperimentCapture {
  private(set) var inFlight = false
  var notice: String?
  var isAskingConsent = false
  var isAskingDeletion = false

  func toggle(_ pane: Pane, client: EngineClient) {
    if pane.experimentCapture?.on == true {
      set(on: false, pane: pane, client: client)
    } else {
      isAskingConsent = true
    }
  }

  func set(on: Bool, pane: Pane, client: EngineClient) {
    inFlight = true
    Task {
      defer { inFlight = false }
      do {
        let result = try await client.setExperimentCapture(paneId: pane.paneId, on: on)
        if result.capture.on {
          notice = nil
        } else {
          notice = "Recording stopped."
          isAskingDeletion = true
        }
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  func deleteData(_ pane: Pane, client: EngineClient) {
    inFlight = true
    Task {
      defer { inFlight = false }
      do {
        let result = try await client.deleteExperimentCaptureData(paneId: pane.paneId)
        notice = ExperimentCapturePresentation.deletionText(result)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}

/// The consent question before a recording starts and the keep-or-delete question after it stops.
private struct ExperimentCaptureDialogs: ViewModifier {
  @Bindable var capture: PaneExperimentCapture
  let pane: Pane
  let client: EngineClient

  private var exportsPath: String {
    DevProfile.dataDirectory
      .appendingPathComponent("experiments/pane-state/exports", isDirectory: true).path
  }

  func body(content: Content) -> some View {
    content
      .confirmationDialog(
        ExperimentCapturePresentation.consentTitle(for: pane.mode),
        isPresented: $capture.isAskingConsent
      ) {
        Button("Start recording") { capture.set(on: true, pane: pane, client: client) }
        Button("Cancel", role: .cancel) {}
      } message: {
        Text(ExperimentCapturePresentation.consentMessage(for: pane.mode))
      }
      .sheet(isPresented: $capture.isAskingDeletion) {
        VStack(alignment: .leading, spacing: MacTheme.Space.x18) {
          Text("Keep the captured experiment data?")
            .font(MacTheme.ui(MacTheme.TypeScale.heading, weight: .semibold))
          Text(
            "Recording has stopped. Delete removes only this pane's recorded segments. Export files remain at \(exportsPath)."
          )
          .font(MacTheme.ui(MacTheme.TypeScale.control))
          .foregroundStyle(MacTheme.secondary)
          HStack {
            Button("Keep data") {
              capture.notice = "Captured data kept on this Mac."
              capture.isAskingDeletion = false
            }
            Spacer()
            Button("Delete captured data", role: .destructive) {
              capture.isAskingDeletion = false
              capture.deleteData(pane, client: client)
            }
            .keyboardShortcut(.defaultAction)
          }
        }
        .padding(MacTheme.Space.x18)
        .macSheetSurface(.captureDeletion)
        .interactiveDismissDisabled()
      }
  }
}

extension View {
  func experimentCaptureDialogs(
    _ capture: PaneExperimentCapture, pane: Pane, client: EngineClient
  ) -> some View {
    modifier(ExperimentCaptureDialogs(capture: capture, pane: pane, client: client))
  }
}
