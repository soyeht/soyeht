import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

private struct PresentationReady {
  let file: EditorFileView
  let plan: WritingPresentationLayoutPlan
}

struct WritingPresentationPane: View {
  @Bindable var client: EngineClient
  @Environment(\.writingSlideShowAction) private var slideShow
  @Environment(\.writingSlideShowViewportRegistry) private var viewportRegistry
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let companionLinked: Bool
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var selectedSlide: UInt32 = 0
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
  @State private var showingOptions = false
  @State private var chrome = WritingChromeModel()

  private var context: WritingPaneContext { WritingPaneContext(client: client, pane: pane) }
  private var slides: [WritingSlide] { context.writing?.structure?.slides ?? [] }

  var body: some View {
    WritingPaneChrome(
      chrome: chrome, clipsPage: true,
      resolve: { size -> WritingResolvedPane<PresentationReady>? in
        guard let document = context.writing?.document, let file = context.file,
          file.viewport != nil,
          let plan = layoutPlan(size: size, document: document),
          case .presentation(let presentationPlan) = plan.surface
        else { return nil }
        return WritingResolvedPane(
          plan: plan, ready: PresentationReady(file: file, plan: presentationPlan))
      },
      header: { _ in presentationHeader },
      surface: { _, ready in plannedSurface(presentationPlan: ready.plan, file: ready.file) }
    )
    .writingPaneLifecycle(context, chrome: chrome)
    .onChange(of: showingOptions) { _, value in
      chrome.send(.menuChanged(value))
    }
  }

  private func layoutPlan(
    size: CGSize, document: WritingDocument
  ) -> WritingLayoutPlan? {
    MacPaneLayoutMetrics.writingPlan(
      kind: .presentation, size: size, font: MacPaneLayoutMetrics.proseMetric(for: document.font))
  }

  @ViewBuilder private func plannedSurface(
    presentationPlan: WritingPresentationLayoutPlan, file: EditorFileView
  ) -> some View {
    if presentationPlan.navigation == .thumbnails {
      slideRail
        .writingFrame(presentationPlan.navigationRect)
        .accessibilityIdentifier("writing-presentation-slide-rail-\(pane.paneId)")
      WritingRailDivider(rail: presentationPlan.navigationRect)
    } else {
      slideNumberStrip
        .writingFrame(presentationPlan.navigationRect)
        .accessibilityIdentifier("writing-presentation-slide-numbers-\(pane.paneId)")
    }
    presentationStage(file)
      .writingFrame(presentationPlan.stageRect)
    footer
      .writingFrame(presentationPlan.footerRect)
  }

  private var presentationHeader: some View {
    WritingChromeHeader(
      paneId: pane.paneId, title: context.documentFileName,
      metadata: context.writing?.structure.map {
        WritingDisplayText.count($0.slides.count, kind: .slide)
      },
      companionLinked: companionLinked, opacity: chrome.opacity,
      headerDrag: headerDrag,
      onHover: { chrome.send(.headerHoverChanged($0)) },
      trailing: {
        WritingOptionsButton(paneId: pane.paneId, name: "Presentation", skipsKeyboardFocus: true) {
          showingOptions.toggle()
        }
        .popover(isPresented: $showingOptions, arrowEdge: .bottom) {
          presentationOptions
        }
      })
  }

  private var presentationOptions: some View {
    WritingOptionsMenu(
      name: "Presentation", paneId: pane.paneId,
      groups: MacPaneLayoutMetrics.chromePolicy.plan(kind: .presentation).menuGroups,
      connected: client.connected, layoutActionsDisabled: layoutActionsDisabled,
      dismiss: { showingOptions = false }, onSplit: onSplit,
      onClose: { chrome.requestClose(context) },
      specific: { action in
        if action == .present {
          WritingOptionButton(action, paneId: pane.paneId, disabled: slides.isEmpty) {
            showingOptions = false
            present()
          }
        }
      }
    )
  }

  private func present() {
    guard let fileId = context.writing?.document.file.fileId else { return }
    let viewportOffset = viewportRegistry?.capture(paneId: pane.paneId)
    let request = WritingSlideShowRequest(
      paneId: pane.paneId, fileId: fileId, initialIndex: Int(selectedSlide),
      slideCount: slides.count, viewportOffset: viewportOffset)
    Task { @MainActor in
      await Task.yield()
      slideShow(request)
    }
  }

  private var slideRail: some View {
    WritingRail(
      title: "SLIDES", count: slides.count, titleSize: MacTheme.TypeScale.micro,
      itemSpacing: MacTheme.Space.x10, contentInset: MacTheme.Space.x4,
      forcesOverlayScrollers: true
    ) {
      ForEach(slides, id: \.index) { slide in
        WritingSlideThumbnail(
          client: client, fileId: context.writing?.document.file.fileId, slide: slide,
          selected: slide.index == selectedSlide
        ) {
          select(slide)
        }
      }
    }
  }

  private var slideNumberStrip: some View {
    ScrollView(.horizontal) {
      HStack(spacing: MacTheme.Space.x6) {
        ForEach(slides, id: \.index) { slide in
          Button {
            select(slide)
          } label: {
            WritingSlideNumber(index: slide.index, selected: slide.index == selectedSlide)
          }
          .buttonStyle(.plain)
          .accessibilityLabel("Slide \(identifier: slide.index + 1)")
          .accessibilityIdentifier(
            "writing-presentation-slide-number-\(pane.paneId)-\(slide.index)")
        }
      }
      .fixedSize(horizontal: true, vertical: false)
      .padding(.horizontal, MacTheme.Space.x10)
      .forceEnclosingOverlayScrollers()
    }
    .scrollIndicators(.hidden)
    .background(MacTheme.face)
  }

  private func presentationStage(_ file: EditorFileView) -> some View {
    WritingPresentationViewport(
      client: client, view: file, activeLine: $activeLine, targetLine: $targetLine,
      paneId: pane.paneId, viewportRegistry: viewportRegistry,
      onEditingActivity: { chrome.send(.edited) }
    )
    .background(MacTheme.face)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    .overlay {
      RoundedRectangle(cornerRadius: MacTheme.Radius.control)
        .stroke(MacTheme.well, lineWidth: 1)
    }
    .accessibilityIdentifier("writing-presentation-stage-\(pane.paneId)")
    .onChange(of: activeLine) { _, line in
      if let slide = slides.first(where: { line >= $0.startLine && line <= $0.endLine }) {
        selectedSlide = slide.index
      }
    }
  }

  private var footer: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "info.circle")
      Text("Title → appears on slide · regular text → your notes · --- → new slide")
      Spacer()
      Text("Markdown")
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.micro))
    .foregroundStyle(MacTheme.muted)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.presentationFooterHeight).background(MacTheme.face)
  }

  private func select(_ slide: WritingSlide) {
    selectedSlide = slide.index
    targetLine = slide.startLine
    context.revealLine(slide.startLine)
  }
}

/// The numbered disc of a slide, in the rail's thumbnail and in the narrow number strip.
private struct WritingSlideNumber: View {
  let index: UInt32
  let selected: Bool

  var body: some View {
    Text("\(identifier: index + 1)")
      .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
      .foregroundStyle(selected ? MacTheme.onAccent : MacTheme.secondary)
      .frame(width: 22, height: 22)
      .background(selected ? MacTheme.accent : MacTheme.well, in: Circle())
  }
}

private struct WritingSlideThumbnail: View {
  @Bindable var client: EngineClient
  let fileId: String?
  let slide: WritingSlide
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
        HStack(alignment: .top) {
          WritingSlideNumber(index: slide.index, selected: selected)
          Spacer()
          if !slide.imagePaths.isEmpty {
            Image(systemName: "photo").foregroundStyle(MacTheme.secondary)
          }
        }
        Text(WritingDisplayText.slideTitle(slide.heading))
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .foregroundStyle(MacTheme.ink).lineLimit(2)
        if let asset = slide.imagePaths.first, let fileId {
          WritingAssetImage(client: client, fileId: fileId, asset: asset)
            .frame(maxWidth: .infinity, minHeight: 34, maxHeight: 44)
        } else {
          Text(slide.notes.isEmpty ? "No speaker notes" : slide.notes)
            .font(MacTheme.chrome(MacTheme.TypeScale.micro))
            .foregroundStyle(MacTheme.muted).lineLimit(2)
        }
      }
      .padding(MacTheme.Space.x10)
      .frame(maxWidth: .infinity, minHeight: 100, alignment: .topLeading)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
      .overlay {
        RoundedRectangle(cornerRadius: MacTheme.Radius.control)
          .stroke(selected ? MacTheme.accent : MacTheme.well, lineWidth: selected ? 2 : 1)
      }
    }
    .buttonStyle(.plain)
  }
}

private struct WritingPresentationViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var activeLine: UInt32
  @Binding var targetLine: UInt32?
  let paneId: String
  let viewportRegistry: WritingPresentationViewportRegistry?
  let onEditingActivity: () -> Void

  func makeCoordinator() -> Coordinator { Coordinator(self) }
  func makeNSView(context: Context) -> NSScrollView {
    let scrollView = context.coordinator.makeScrollView()
    viewportRegistry?.attach(scrollView, paneId: paneId)
    return scrollView
  }
  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    viewportRegistry?.attach(scrollView, paneId: paneId)
    context.coordinator.update()
  }

  static func dismantleNSView(_ scrollView: NSScrollView, coordinator: Coordinator) {
    coordinator.parent.viewportRegistry?.detach(scrollView, paneId: coordinator.parent.paneId)
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    var parent: WritingPresentationViewport
    private let textView = WritingImagePasteTextView()
    private var text = WritingViewportText()
    private var revision: String?
    private var renderedTheme: TerminalColorTheme?
    private var applying = false

    init(_ parent: WritingPresentationViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView.writingScrollView(holding: textView)
      configure()
      return scroll
    }

    func update() {
      guard let viewport = parent.view.viewport else { return }
      textView.applyWritingPalette()
      let next = WritingViewportText(lines: viewport.lines, firstLine: viewport.fromLine)
      let nextRevision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if nextRevision != revision || renderedTheme != SoyehtThemeStore.shared.activeTheme {
        text = next
        revision = nextRevision
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: text.clamped(textView.selectedRange()))
      }
      if let line = parent.targetLine, line >= viewport.fromLine, line < viewport.toLine {
        scroll(to: line)
        parent.targetLine = nil
      }
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !applying, let position = text.position(utf16Offset: textView.selectedRange().location)
      else { return }
      parent.activeLine = position.line
    }

    func textView(
      _ textView: NSTextView, shouldChangeTextIn range: NSRange, replacementString: String?
    ) -> Bool {
      let replacement = replacementString ?? ""
      guard let change = text.replace(range, with: replacement) else { return false }
      parent.onEditingActivity()
      parent.client.enqueueEditorEdits(fileId: parent.view.file.fileId, edits: [change.edit])
      parent.activeLine =
        change.edit.range.start.line &+ UInt32(clamping: replacement.filter { $0 == "\n" }.count)
      render(selection: NSRange(location: change.cursor, length: 0))
      return false
    }

    private func render(selection: NSRange) {
      applying = true
      textView.textStorage?.setAttributedString(WritingPresentationStyle.attributed(text.source))
      textView.setSelectedRange(text.clamped(selection))
      textView.needsDisplay = true
      applying = false
    }

    private func scroll(to line: UInt32) {
      guard let location = text.utf16Location(ofLine: line) else { return }
      textView.placeCaret(at: location)
      parent.activeLine = line
    }

    private func configure() {
      textView.configureWritingEditing(delegate: self, checksSpelling: false)
      textView.configureWritingColumn(inset: NSSize(width: 48, height: 36))
      textView.acceptImages { [weak self] pasteboard in
        guard let self else { return false }
        return WritingImageInsertion.insert(
          from: pasteboard, in: textView, text: text, client: parent.client,
          fileId: parent.view.file.fileId)
      }
    }
  }
}

@MainActor private enum WritingPresentationStyle {
  static func attributed(_ source: String) -> NSAttributedString {
    let paragraph = NSMutableParagraphStyle()
    paragraph.minimumLineHeight = 23
    paragraph.maximumLineHeight = 23
    let result = NSMutableAttributedString(
      string: source,
      attributes: [
        .font: MacTheme.monoNSFont(12), .foregroundColor: NSColor(MacTheme.ink),
        .paragraphStyle: paragraph,
      ])
    var location = 0
    for line in source.components(separatedBy: "\n") {
      let range = NSRange(location: location, length: line.utf16.count)
      if line.hasPrefix("#") {
        result.addAttributes(
          [.foregroundColor: NSColor(MacTheme.accent), .font: MacTheme.monoNSFont(12, bold: true)],
          range: range)
      } else if line.trimmingCharacters(in: .whitespaces) == "---" {
        result.addAttribute(.foregroundColor, value: NSColor(MacTheme.muted), range: range)
      } else if line.trimmingCharacters(in: .whitespaces).hasPrefix("![") {
        result.addAttributes(
          [.foregroundColor: NSColor(MacTheme.success), .backgroundColor: NSColor(MacTheme.well)],
          range: range)
      }
      location += line.utf16.count + 1
    }
    return result
  }
}
