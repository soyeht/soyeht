import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

/// A phone presentation keeps the stage at 16:9 and lets the shared writing policy
/// choose thumbnails or the compact numeric strip from the measured viewport.
@MainActor
struct PhoneWritingPresentationSurface: View {
  @Bindable var client: EngineClient
  let writing: WritingDocumentView
  let file: EditorFileView
  let structure: WritingStructureResult
  let size: PaneLayoutSize
  let font: PaneLayoutFontMetric
  let floors: [PaneLayoutKind: PaneLayoutSize]
  let accessibilityName: String

  @State private var selectedSlide: UInt32 = 0

  private var slides: [WritingSlide] { structure.slides }

  private var plan: WritingLayoutPlan? {
    try? WritingLayoutPolicy(
      floors: floors, gaps: PaneLayoutGaps(horizontal: 18, vertical: 18)
    ).layout(
      WritingLayoutRequest(
        kind: .presentation,
        viewport: PaneLayoutSize(
          width: size.width,
          height: size.height + WritingLayoutMetrics.standard.headerHeight),
        font: font))
  }

  var body: some View {
    if let plan, case .presentation(let presentation) = plan.surface {
      ScrollView(scrollAxes(plan), showsIndicators: !plan.overflowAxes.isEmpty) {
        ZStack(alignment: .topLeading) {
          PhonePalette.canvas
            .frame(width: plan.contentSize.width, height: contentHeight(plan))
          if presentation.navigation == .thumbnails {
            thumbnailRail
              .phoneWritingFrame(translated(presentation.navigationRect))
              .accessibilityIdentifier(
                "phone-writing-presentation-thumbnails-\(writing.document.file.paneId)")
          } else {
            numberStrip
              .phoneWritingFrame(translated(presentation.navigationRect))
              .accessibilityIdentifier(
                "phone-writing-presentation-numbers-\(writing.document.file.paneId)")
          }
          stage
            .phoneWritingFrame(translated(presentation.stageRect))
          footer
            .phoneWritingFrame(translated(presentation.footerRect))
        }
        .frame(
          width: plan.contentSize.width, height: contentHeight(plan), alignment: .topLeading)
      }
      .scrollDisabled(plan.overflowAxes.isEmpty)
      .scrollBounceBehavior(.basedOnSize)
      .frame(width: size.width, height: size.height)
      .onAppear { synchronizeSelection() }
      .onChange(of: file.viewport?.fromLine) { _, _ in synchronizeSelection() }
      .onChange(of: slides.map(\.index)) { _, _ in synchronizeSelection() }
    }
  }

  private var thumbnailRail: some View {
    ScrollView(.vertical) {
      LazyVStack(spacing: 10) {
        ForEach(slides, id: \.index) { slide in
          Button {
            select(slide)
          } label: {
            VStack(alignment: .leading, spacing: 7) {
              HStack {
                Text("\(slide.index + 1)")
                  .font(.caption.monospaced().bold())
                Spacer()
                if !slide.imagePaths.isEmpty { Image(systemName: "photo") }
              }
              Text(slide.heading.isEmpty ? "Untitled slide" : slide.heading)
                .font(.subheadline.bold())
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(10)
            .frame(maxWidth: .infinity, minHeight: 88, alignment: .topLeading)
            .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 10))
            .overlay {
              RoundedRectangle(cornerRadius: 10)
                .stroke(
                  slide.index == selectedSlide
                    ? PhonePalette.accent : PhonePalette.secondary.opacity(0.2),
                  lineWidth: slide.index == selectedSlide ? 2 : 1)
            }
          }
          .buttonStyle(.plain)
          .accessibilityLabel(slideAccessibilityLabel(slide))
          .accessibilityValue(slide.index == selectedSlide ? "Selected" : "Not selected")
          .accessibilityAddTraits(slide.index == selectedSlide ? .isSelected : [])
          .accessibilityIdentifier(
            "phone-writing-slide-thumbnail-\(writing.document.file.paneId)-\(slide.index)")
        }
      }
      .padding(8)
    }
    .background(PhonePalette.canvas)
  }

  private var numberStrip: some View {
    ScrollView(.horizontal) {
      HStack(spacing: 8) {
        ForEach(slides, id: \.index) { slide in
          Button {
            select(slide)
          } label: {
            Text("\(slide.index + 1)")
              .font(.caption.monospaced().bold())
              .foregroundStyle(
                slide.index == selectedSlide ? Color.white : PhonePalette.secondary
              )
              .frame(width: 44, height: 44)
              .background(
                slide.index == selectedSlide ? PhonePalette.accent : PhonePalette.face,
                in: Circle())
          }
          .buttonStyle(.plain)
          .accessibilityLabel(slideAccessibilityLabel(slide))
          .accessibilityValue(slide.index == selectedSlide ? "Selected" : "Not selected")
          .accessibilityAddTraits(slide.index == selectedSlide ? .isSelected : [])
          .accessibilityIdentifier(
            "phone-writing-slide-number-\(writing.document.file.paneId)-\(slide.index)")
        }
      }
      .padding(.horizontal, 10)
    }
    .scrollIndicators(.hidden)
    .background(PhonePalette.canvas)
  }

  private var stage: some View {
    ZStack {
      PhonePalette.face
      if let slide = selected {
        VStack(spacing: 14) {
          Text(slide.heading.isEmpty ? "Untitled slide" : slide.heading)
            .font(.system(.title2, design: .rounded, weight: .bold))
            .multilineTextAlignment(.center)
            .lineLimit(3)
            .minimumScaleFactor(0.55)
          if !slide.imagePaths.isEmpty {
            HStack(spacing: 10) {
              ForEach(Array(slide.imagePaths.prefix(2)), id: \.self) { asset in
                PhoneWritingPresentationAsset(
                  client: client, fileId: writing.document.file.fileId,
                  revision: writing.document.file.revision, asset: asset)
              }
            }
          }
        }
        .padding(20)
      } else {
        ContentUnavailableView("No slides", systemImage: "rectangle.on.rectangle")
      }
    }
    .clipShape(RoundedRectangle(cornerRadius: 12))
    .overlay {
      RoundedRectangle(cornerRadius: 12)
        .stroke(PhonePalette.secondary.opacity(0.22), lineWidth: 1)
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(stageAccessibilityLabel)
    .accessibilityIdentifier("phone-writing-presentation-stage-\(writing.document.file.paneId)")
  }

  private var footer: some View {
    HStack(spacing: 8) {
      Image(systemName: "rectangle.ratio.16.to.9")
      Text(slides.isEmpty ? "No slides" : "Slide \(selectedPosition) of \(slides.count)")
      Spacer()
      Text("16:9")
    }
    .font(.caption)
    .foregroundStyle(PhonePalette.secondary)
    .padding(.horizontal, 10)
    .background(PhonePalette.canvas)
    .accessibilityElement(children: .combine)
  }

  private var selected: WritingSlide? {
    slides.first { $0.index == selectedSlide } ?? slides.first
  }

  private var selectedPosition: Int {
    guard let index = slides.firstIndex(where: { $0.index == selectedSlide }) else { return 1 }
    return index + 1
  }

  private var stageAccessibilityLabel: String {
    guard let selected else { return "\(accessibilityName), presentation, no slides" }
    return "\(accessibilityName), presentation, slide \(selected.index + 1), "
      + (selected.heading.isEmpty ? "Untitled slide" : selected.heading)
  }

  private func slideAccessibilityLabel(_ slide: WritingSlide) -> String {
    "Slide \(slide.index + 1), " + (slide.heading.isEmpty ? "Untitled slide" : slide.heading)
  }

  private func select(_ slide: WritingSlide) {
    selectedSlide = slide.index
    guard let viewport = file.viewport,
      slide.startLine < viewport.fromLine || slide.startLine >= viewport.toLine
    else { return }
    Task { @MainActor in
      do {
        try await client.loadEditorViewport(
          fileId: writing.document.file.fileId, fromLine: slide.startLine)
      } catch {
        client.problem = "Could not open that slide."
      }
    }
  }

  private func synchronizeSelection() {
    guard !slides.isEmpty else {
      selectedSlide = 0
      return
    }
    if let line = file.viewport?.fromLine,
      let slide = slides.first(where: { line >= $0.startLine && line <= $0.endLine })
    {
      selectedSlide = slide.index
    } else if !slides.contains(where: { $0.index == selectedSlide }) {
      selectedSlide = slides[0].index
    }
  }

  private func scrollAxes(_ plan: WritingLayoutPlan) -> Axis.Set {
    var result: Axis.Set = []
    if plan.overflowAxes.contains(.horizontal) { result.insert(.horizontal) }
    if plan.overflowAxes.contains(.vertical) { result.insert(.vertical) }
    return result
  }

  private func contentHeight(_ plan: WritingLayoutPlan) -> Double {
    max(0, plan.contentSize.height - WritingLayoutMetrics.standard.headerHeight)
  }

  private func translated(_ rect: PaneLayoutRect) -> PaneLayoutRect {
    PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: rect.origin.x,
        y: rect.origin.y - WritingLayoutMetrics.standard.headerHeight),
      size: rect.size)
  }
}

@MainActor
private struct PhoneWritingPresentationAsset: View {
  @Bindable var client: EngineClient
  let fileId: String
  let revision: String
  let asset: String
  @State private var data: Data?
  @State private var failed = false

  var body: some View {
    Group {
      if let data, let image = UIImage(data: data) {
        Image(uiImage: image)
          .resizable()
          .scaledToFit()
      } else if failed {
        Image(systemName: "photo.badge.exclamationmark")
          .font(.title2)
          .foregroundStyle(PhonePalette.secondary)
          .accessibilityLabel("Image unavailable")
      } else {
        ProgressView()
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .task(id: "\(revision):\(asset)") {
      data = nil
      failed = false
      do {
        data = try await client.readWritingAsset(fileId: fileId, asset: asset)
      } catch {
        data = nil
        failed = true
      }
    }
  }
}
