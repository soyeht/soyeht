import SoyehtClient
import SoyehtProtocol
import SwiftUI

@MainActor
struct PhoneWritingTextSurface: View {
  @Bindable var client: EngineClient
  let pane: Pane?
  let writing: WritingDocumentView
  let file: EditorFileView?
  let size: PaneLayoutSize
  let font: PaneLayoutFontMetric
  let accessibilityName: String
  let onEditingActivity: () -> Void
  let onSelectionActivity: (Bool) -> Void
  let onScrollActivity: () -> Void

  @State private var destinationRequest: PhoneWritingDestinationRequest?
  @State private var destination = ""

  private var plan: WritingLayoutPlan? {
    try? WritingLayoutPolicy(
      metrics: PhoneWritingMetrics.bodyWritingMetrics,
      floors: PhoneWritingMetrics.bodyFloors, gaps: PhoneWritingMetrics.sizing.gaps
    ).layout(
      WritingLayoutRequest(
        kind: .text,
        viewport: size,
        font: font, fullWidth: writing.document.fullWidth,
        reviewVisible: file?.review != nil))
  }

  var body: some View {
    if let plan, case .text(let textPlan) = plan.surface {
      ZStack(alignment: .topLeading) {
        PhonePalette.face
        if let review = plan.reviewRect {
          reviewView.writingFrame(review)
        }
        if let file, file.viewport != nil {
          PhoneWritingTextEditor(
            client: client, view: file, document: writing.document,
            accessibilityName: accessibilityName,
            onEditingActivity: onEditingActivity,
            onSelectionActivity: onSelectionActivity,
            onScrollActivity: onScrollActivity,
            onFormat: { kind, range in apply(kind, range: range) },
            onDestination: { kind, range in
              destination = ""
              destinationRequest = PhoneWritingDestinationRequest(kind: kind, range: range)
            }
          )
          .writingFrame(textPlan.columnRect)
          .clipShape(RoundedRectangle(cornerRadius: DesignTokens.Radius.compact))
          .accessibilityIdentifier("phone-writing-text-\(writing.document.file.paneId)")
        } else {
          ProgressView().writingFrame(textPlan.columnRect)
        }
      }
      .frame(width: size.width, height: size.height, alignment: .topLeading)
      .sheet(item: $destinationRequest) { request in
        let sheetKind: PhoneWritingSheetKind = request.kind == .image ? .image : .link
        NavigationStack {
          Form {
            TextField(
              "", text: $destination,
              prompt: Text(request.kind == .image ? "Image path" : "Link destination")
                .foregroundStyle(PhonePalette.secondary)
            )
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .phoneWritingSheetRow()
          }
          .phoneWritingSheetScroll()
          .phoneWritingSheetNavigation(request.kind == .image ? "Insert image" : "Add link")
          .toolbar {
            ToolbarItem(placement: .cancellationAction) {
              Button("Cancel") { destinationRequest = nil }
            }
            ToolbarItem(placement: .confirmationAction) {
              Button("Apply") {
                apply(request.kind, range: request.range, value: destination)
                destinationRequest = nil
              }
              .disabled(destination.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
          }
        }
        .phoneWritingSheet(sheetKind)
      }
    }
  }

  private var reviewView: some View {
    Label("Agent changes ready to review", systemImage: "person.crop.circle.badge.checkmark")
      .font(.caption.bold())
      .foregroundStyle(PhonePalette.secondary)
      .padding(.horizontal, DesignTokens.Space.x10)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
      .background(
        PhonePalette.face, in: RoundedRectangle(cornerRadius: DesignTokens.Radius.compact))
  }

  private func apply(
    _ kind: WritingFormatKind, range: SoyehtProtocol.TextRange, value: String? = nil
  ) {
    let fileId = writing.document.file.fileId
    Task { @MainActor in
      do {
        _ = try await client.formatWritingDocument(
          fileId: fileId, range: range, kind: kind, value: value)
      } catch {
        client.problem = "Could not format the selected text."
      }
    }
  }

}

struct PhoneWritingDestinationRequest: Identifiable {
  let id = UUID()
  let kind: WritingFormatKind
  let range: SoyehtProtocol.TextRange
}
