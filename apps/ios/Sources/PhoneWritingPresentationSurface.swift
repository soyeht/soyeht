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
  let accessibilityName: String
  @Binding var selectedSlide: UInt32
  let onActivity: () -> Void

  private var slides: [WritingSlide] { structure.slides }
  private var chromePlan: PhoneWritingChromePlan {
    PhoneWritingMetrics.chromePolicy.plan(kind: .presentation)
  }

  private var plan: WritingLayoutPlan? {
    try? WritingLayoutPolicy(
      metrics: PhoneWritingMetrics.bodyWritingMetrics,
      floors: PhoneWritingMetrics.bodyFloors, gaps: PhoneWritingMetrics.sizing.gaps
    ).layout(
      WritingLayoutRequest(
        kind: .presentation,
        viewport: size,
        font: font))
  }

  var body: some View {
    if let plan, case .presentation(let presentation) = plan.surface {
      ScrollView(scrollAxes(plan), showsIndicators: !plan.overflowAxes.isEmpty) {
        ZStack(alignment: .topLeading) {
          PhonePalette.face
            .frame(width: plan.contentSize.width, height: contentHeight(plan))
          if presentation.navigation == .thumbnails {
            thumbnailRail
              .phoneWritingFrame(presentation.navigationRect)
              .accessibilityIdentifier(
                "phone-writing-presentation-thumbnails-\(writing.document.file.paneId)")
          } else {
            numberStrip
              .phoneWritingFrame(presentation.navigationRect)
              .accessibilityIdentifier(
                "phone-writing-presentation-numbers-\(writing.document.file.paneId)")
          }
          stage
            .phoneWritingFrame(presentation.stageRect)
          footer
            .phoneWritingFrame(presentation.footerRect)
        }
        .frame(
          width: plan.contentSize.width, height: contentHeight(plan), alignment: .topLeading)
      }
      .scrollDisabled(plan.overflowAxes.isEmpty)
      .scrollBounceBehavior(.basedOnSize)
      .frame(width: size.width, height: size.height)
      .simultaneousGesture(
        DragGesture(minimumDistance: 3).onChanged { _ in onActivity() }
      )
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
            onActivity()
            select(slide)
          } label: {
            VStack(alignment: .leading, spacing: 7) {
              HStack {
                Text("\(slide.index + 1)")
                  .font(.caption.monospaced().bold())
                Spacer()
                if !slide.imagePaths.isEmpty { Image(systemName: "photo") }
              }
              Text(WritingDisplayText.slideTitle(slide.heading))
                .font(.subheadline.bold())
                .foregroundStyle(PhonePalette.ink)
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
    .background(PhonePalette.face)
  }

  private var numberStrip: some View {
    ScrollView(.horizontal) {
      HStack(spacing: 8) {
        ForEach(slides, id: \.index) { slide in
          Button {
            onActivity()
            select(slide)
          } label: {
            Text("\(slide.index + 1)")
              .font(.caption.monospaced().bold())
              .foregroundStyle(
                slide.index == selectedSlide ? Color.white : PhonePalette.secondary
              )
              .frame(
                width: chromePlan.minimumActionTarget,
                height: chromePlan.minimumActionTarget
              )
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
    .background(PhonePalette.face)
  }

  private var stage: some View {
    ZStack {
      PhonePalette.face
      if let slide = selected {
        VStack(spacing: 14) {
          Text(WritingDisplayText.slideTitle(slide.heading))
            .font(.system(.title2, design: .rounded, weight: .bold))
            .foregroundStyle(PhonePalette.ink)
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
    .background(PhonePalette.face)
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
      + WritingDisplayText.slideTitle(selected.heading)
  }

  private func slideAccessibilityLabel(_ slide: WritingSlide) -> String {
    "Slide \(slide.index + 1), " + WritingDisplayText.slideTitle(slide.heading)
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
    plan.contentSize.height
  }
}

@MainActor
struct PhoneWritingPresentationPlayer: View {
  @Bindable var client: EngineClient
  let writing: WritingDocumentView
  let slides: [WritingSlide]
  let accessibilityName: String

  @Environment(\.dismiss) private var dismiss
  @State private var index: Int

  private var chromePlan: PhoneWritingChromePlan {
    PhoneWritingMetrics.chromePolicy.plan(kind: .presentation)
  }

  init(
    client: EngineClient, writing: WritingDocumentView, slides: [WritingSlide],
    initialSlide: UInt32, accessibilityName: String
  ) {
    self.client = client
    self.writing = writing
    self.slides = slides
    self.accessibilityName = accessibilityName
    _index = State(
      initialValue: slides.firstIndex(where: { $0.index == initialSlide }) ?? 0)
  }

  var body: some View {
    NavigationStack {
      GeometryReader { proxy in
        ZStack {
          Color.black.ignoresSafeArea()
          if let slide {
            VStack(spacing: 18) {
              Text(WritingDisplayText.slideTitle(slide.heading))
                .font(.system(.largeTitle, design: .rounded, weight: .bold))
                .foregroundStyle(PhonePalette.ink)
                .multilineTextAlignment(.center)
                .lineLimit(4)
                .minimumScaleFactor(0.45)
              if !slide.imagePaths.isEmpty {
                HStack(spacing: 12) {
                  ForEach(Array(slide.imagePaths.prefix(2)), id: \.self) { asset in
                    PhoneWritingPresentationAsset(
                      client: client, fileId: writing.document.file.fileId,
                      revision: writing.document.file.revision, asset: asset)
                  }
                }
              }
            }
            .padding(24)
            .frame(
              width: min(proxy.size.width, proxy.size.height * 16 / 9),
              height: min(proxy.size.height, proxy.size.width * 9 / 16)
            )
            .background(PhonePalette.face)
            .foregroundStyle(PhonePalette.ink)
            .clipShape(RoundedRectangle(cornerRadius: 14))
            .accessibilityElement(children: .contain)
            .accessibilityLabel(
              "\(accessibilityName), slide \(slide.index + 1), "
                + WritingDisplayText.slideTitle(slide.heading))
          } else {
            ContentUnavailableView("No slides", systemImage: "rectangle.on.rectangle")
              .foregroundStyle(.white)
          }
        }
      }
      .safeAreaInset(edge: .bottom) {
        HStack(spacing: 12) {
          Button {
            index = max(0, index - 1)
          } label: {
            PhoneWritingActionLabel(
              title: "Previous", systemImage: "chevron.left",
              minimumTarget: chromePlan.minimumActionTarget)
          }
          .disabled(index == 0 || slides.isEmpty)
          Spacer()
          Text(slides.isEmpty ? "0 / 0" : "\(index + 1) / \(slides.count)")
            .font(.subheadline.monospacedDigit())
          Spacer()
          Button {
            index = min(slides.count - 1, index + 1)
          } label: {
            PhoneWritingActionLabel(
              title: "Next", systemImage: "chevron.right",
              minimumTarget: chromePlan.minimumActionTarget)
          }
          .disabled(slides.isEmpty || index >= slides.count - 1)
        }
        .frame(minHeight: chromePlan.minimumActionTarget)
        .padding(.horizontal, 18)
        .background(.ultraThinMaterial)
      }
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button {
            dismiss()
          } label: {
            PhoneWritingActionLabel(
              title: "Done", minimumTarget: chromePlan.minimumActionTarget)
          }
        }
      }
    }
    .tint(PhonePalette.accent)
  }

  private var slide: WritingSlide? {
    guard slides.indices.contains(index) else { return nil }
    return slides[index]
  }
}

struct PhoneWritingActionLabel: View {
  let title: String
  var systemImage: String?
  let minimumTarget: Double

  var body: some View {
    Group {
      if let systemImage {
        Label(title, systemImage: systemImage)
      } else {
        Text(title)
      }
    }
    .frame(minWidth: minimumTarget, minHeight: minimumTarget)
    .contentShape(Rectangle())
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
