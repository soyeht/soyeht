import SoyehtClient
import SoyehtProtocol
import SwiftUI

@MainActor
struct PhoneWritingScenes: View {
  @Bindable var client: EngineClient
  let writing: WritingDocumentView
  let file: EditorFileView
  let structure: WritingStructureResult
  let accessibilityName: String

  @Environment(\.dismiss) private var dismiss
  private var sheetPlan: PhoneWritingSheetPlan {
    PhoneWritingMetrics.sheetPolicy.plan(for: .scenes)
  }

  var body: some View {
    NavigationStack {
      List(structure.scenes, id: \.sceneId) { scene in
        Button {
          open(scene)
        } label: {
          HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text("\(scene.ordinal + 1)")
              .font(.subheadline.monospacedDigit().bold())
              .foregroundStyle(PhonePalette.muted)
            Text(WritingDisplayText.sceneTitle(scene.heading))
              .lineLimit(2)
              .foregroundStyle(PhonePalette.ink)
          }
          .frame(minHeight: sheetPlan.minimumActionTarget)
        }
        .accessibilityLabel(
          "Scene \(scene.ordinal + 1), "
            + WritingDisplayText.sceneTitle(scene.heading)
        )
        .phoneWritingSheetRow()
      }
      .phoneWritingSheetScroll()
      .phoneWritingSheetNavigation("Scenes")
      .phoneWritingDoneToolbar(minimumTarget: sheetPlan.minimumActionTarget, dismiss: dismiss)
    }
    .phoneWritingSheet(.scenes)
    .accessibilityLabel("\(accessibilityName), scenes")
  }

  private func open(_ scene: ScreenplayScene) {
    Task { @MainActor in
      do {
        try await client.loadEditorViewport(
          fileId: writing.document.file.fileId, fromLine: scene.line)
        dismiss()
      } catch {
        client.problem = "Could not open that scene."
      }
    }
  }
}

@MainActor
struct PhoneWritingIdeaBoard: View {
  @Bindable var client: EngineClient
  let writing: WritingDocumentView
  let structure: WritingStructureResult
  let accessibilityName: String

  @Environment(\.dismiss) private var dismiss
  private var sheetPlan: PhoneWritingSheetPlan {
    PhoneWritingMetrics.sheetPolicy.plan(for: .ideaBoard)
  }

  var body: some View {
    NavigationStack {
      Group {
        if structure.beats.isEmpty {
          VStack(spacing: 8) {
            Image(systemName: "rectangle.3.group")
              .font(.title)
              .foregroundStyle(PhonePalette.muted)
            Text("No ideas")
              .font(.headline)
              .foregroundStyle(PhonePalette.ink)
          }
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .background(PhonePalette.canvas)
        } else {
          List(structure.beats.sorted { $0.order < $1.order }, id: \.beatId) { beat in
            VStack(alignment: .leading, spacing: 5) {
              Text(beat.title.isEmpty ? "Untitled idea" : beat.title)
                .font(.headline)
                .foregroundStyle(PhonePalette.ink)
              if !beat.body.isEmpty {
                Text(beat.body)
                  .font(.subheadline)
                  .foregroundStyle(PhonePalette.secondary)
                  .lineLimit(3)
              }
              Text(String(describing: beat.category).capitalized)
                .font(.caption)
                .foregroundStyle(PhonePalette.muted)
            }
            .frame(minHeight: sheetPlan.minimumActionTarget, alignment: .leading)
            .phoneWritingSheetRow()
          }
          .phoneWritingSheetScroll()
        }
      }
      .background(PhonePalette.canvas)
      .phoneWritingSheetNavigation("Idea board")
      .phoneWritingDoneToolbar(minimumTarget: sheetPlan.minimumActionTarget, dismiss: dismiss)
    }
    .phoneWritingSheet(.ideaBoard)
    .accessibilityLabel("\(accessibilityName), idea board")
  }
}
