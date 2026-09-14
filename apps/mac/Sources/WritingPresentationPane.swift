import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingPresentationPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @State private var selectedSlide: UInt32 = 0
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
  @State private var presenting = false

  private var writing: WritingDocumentView? {
    client.writingDocuments.values.first { $0.document.file.paneId == pane.paneId }
  }
  private var file: EditorFileView? {
    guard let fileId = writing?.document.file.fileId else { return nil }
    return client.editorFiles[fileId]
  }
  private var slides: [WritingSlide] { writing?.structure?.slides ?? [] }

  var body: some View {
    VStack(spacing: 0) {
      header
      if let file, file.viewport != nil {
        HStack(spacing: 0) {
          slideRail
          Divider().overlay(MacTheme.well)
          sourceEditor(file)
        }
      } else {
        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    .task(id: writing?.document.file.fileId) { await loadIfNeeded() }
    .sheet(isPresented: $presenting) {
      WritingSlideShow(slides: slides, initialIndex: selectedSlide)
    }
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "rectangle.on.rectangle.angled")
        .foregroundStyle(MacTheme.secondary)
      Text(pane.name)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold)).lineLimit(1)
      Spacer()
      Text("\(slides.count) slides")
        .font(MacTheme.chrome(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.muted)
      Button {
        presenting = !slides.isEmpty
      } label: {
        Label("Present", systemImage: "play.fill")
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      }
      .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(slides.isEmpty)
    }
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 42).background(MacTheme.face)
  }

  private var slideRail: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      HStack {
        Text("SLIDES")
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
        Spacer()
        Text("\(slides.count)")
          .font(MacTheme.mono(MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
      }
      ScrollView {
        LazyVStack(spacing: MacTheme.Space.x10) {
          ForEach(slides, id: \.index) { slide in
            WritingSlideThumbnail(slide: slide, selected: slide.index == selectedSlide) {
              select(slide)
            }
          }
        }
        .padding(MacTheme.Space.x4)
      }
    }
    .padding(MacTheme.Space.x12).frame(width: 236).background(MacTheme.canvas)
  }

  private func sourceEditor(_ file: EditorFileView) -> some View {
    VStack(spacing: 0) {
      WritingPresentationViewport(
        client: client, view: file, activeLine: $activeLine, targetLine: $targetLine
      )
      footer
    }
    .background(MacTheme.face)
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
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 28).background(MacTheme.canvas)
  }

  private func select(_ slide: WritingSlide) {
    selectedSlide = slide.index
    targetLine = slide.startLine
    guard let fileId = writing?.document.file.fileId,
      let viewport = file?.viewport,
      slide.startLine < viewport.fromLine || slide.startLine >= viewport.toLine
    else { return }
    Task { @MainActor in
      do { try await client.loadEditorViewport(fileId: fileId, fromLine: slide.startLine) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func loadIfNeeded() async {
    guard let writing else { return }
    do { try await client.loadWritingPane(writing.document.file.paneId) } catch {
      client.problem = error.localizedDescription
    }
  }
}

private struct WritingSlideThumbnail: View {
  let slide: WritingSlide
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
        HStack(alignment: .top) {
          Text("\(slide.index + 1)")
            .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
            .foregroundStyle(selected ? MacTheme.onAccent : MacTheme.secondary)
            .frame(width: 22, height: 22)
            .background(selected ? MacTheme.accent : MacTheme.well, in: Circle())
          Spacer()
          if !slide.imagePaths.isEmpty {
            Image(systemName: "photo").foregroundStyle(MacTheme.secondary)
          }
        }
        Text(slide.heading.isEmpty ? "Untitled slide" : slide.heading)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .foregroundStyle(MacTheme.ink).lineLimit(2)
        Text(slide.notes.isEmpty ? "No speaker notes" : slide.notes)
          .font(MacTheme.chrome(MacTheme.TypeScale.micro))
          .foregroundStyle(MacTheme.muted).lineLimit(2)
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

private struct WritingSlideShow: View {
  let slides: [WritingSlide]
  @State private var index: Int
  @Environment(\.dismiss) private var dismiss

  init(slides: [WritingSlide], initialIndex: UInt32) {
    self.slides = slides
    _index = State(initialValue: min(Int(initialIndex), max(slides.count - 1, 0)))
  }

  var body: some View {
    VStack(spacing: MacTheme.Space.x18) {
      HStack {
        Text("\(index + 1) / \(slides.count)")
          .font(MacTheme.mono(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.muted)
        Spacer()
        Button("Done") { dismiss() }.buttonStyle(NeoPillButtonStyle(accent: true))
      }
      if let slide = slides[safe: index] {
        VStack(spacing: 28) {
          Text(slide.heading.isEmpty ? "Untitled slide" : slide.heading)
            .font(MacTheme.prose(.sans, 46, weight: .bold)).multilineTextAlignment(.center)
          ForEach(slide.imagePaths, id: \.self) { path in
            Label(path, systemImage: "photo")
              .font(MacTheme.mono(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.secondary)
          }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
      }
      HStack {
        Button("Previous") { index = max(index - 1, 0) }.disabled(index == 0)
        Spacer()
        Button("Next") { index = min(index + 1, slides.count - 1) }
          .disabled(index + 1 >= slides.count)
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
    }
    .padding(32).frame(minWidth: 900, minHeight: 620).background(MacTheme.canvas)
  }
}

private struct WritingPresentationViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var activeLine: UInt32
  @Binding var targetLine: UInt32?

  func makeCoordinator() -> Coordinator { Coordinator(self) }
  func makeNSView(context: Context) -> NSScrollView { context.coordinator.makeScrollView() }
  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update()
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    var parent: WritingPresentationViewport
    private let textView = NSTextView()
    private var source = ""
    private var firstLine: UInt32 = 0
    private var revision: String?
    private var applying = false

    init(_ parent: WritingPresentationViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.autohidesScrollers = true
      scroll.documentView = textView
      configure()
      return scroll
    }

    func update() {
      guard let viewport = parent.view.viewport else { return }
      firstLine = viewport.fromLine
      let next = viewport.lines.map(\.text).joined(separator: "\n")
      let nextRevision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if nextRevision != revision {
        source = next
        revision = nextRevision
        render(selection: valid(textView.selectedRange()))
      }
      if let line = parent.targetLine, line >= viewport.fromLine, line < viewport.toLine {
        scroll(to: line)
        parent.targetLine = nil
      }
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !applying, let position = position(textView.selectedRange().location) else { return }
      parent.activeLine = position.line
    }

    func textView(
      _ textView: NSTextView, shouldChangeTextIn range: NSRange, replacementString: String?
    ) -> Bool {
      let replacement = replacementString ?? ""
      let raw = source as NSString
      guard NSMaxRange(range) <= raw.length,
        let start = position(range.location), let end = position(NSMaxRange(range))
      else { return false }
      source = raw.replacingCharacters(in: range, with: replacement)
      let cursor = range.location + replacement.utf16.count
      parent.client.enqueueEditorEdits(
        fileId: parent.view.file.fileId,
        edits: [TextEdit(range: .init(start: start, end: end), text: replacement)])
      parent.activeLine = start.line &+ UInt32(clamping: replacement.filter { $0 == "\n" }.count)
      render(selection: NSRange(location: cursor, length: 0))
      return false
    }

    private func render(selection: NSRange) {
      applying = true
      textView.textStorage?.setAttributedString(WritingPresentationStyle.attributed(source))
      textView.setSelectedRange(valid(selection))
      textView.needsDisplay = true
      applying = false
    }

    private func scroll(to line: UInt32) {
      let relative = Int(line - firstLine)
      let lines = source.components(separatedBy: "\n")
      guard relative >= 0, relative < lines.count else { return }
      let location = lines.prefix(relative).reduce(0) { $0 + $1.utf16.count + 1 }
      textView.setSelectedRange(NSRange(location: location, length: 0))
      textView.scrollRangeToVisible(NSRange(location: location, length: 0))
      parent.activeLine = line
    }

    private func position(_ offset: Int) -> TextPosition? {
      EditorEditing.textPosition(text: source, viewportStartLine: firstLine, utf16Offset: offset)
    }

    private func valid(_ range: NSRange) -> NSRange {
      let length = source.utf16.count
      guard range.location != NSNotFound, range.location <= length else { return .init() }
      return NSRange(location: range.location, length: min(range.length, length - range.location))
    }

    private func configure() {
      textView.delegate = self
      textView.isRichText = false
      textView.importsGraphics = false
      textView.allowsUndo = true
      textView.usesFindPanel = true
      textView.drawsBackground = true
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
      textView.textContainerInset = NSSize(width: 48, height: 36)
      textView.minSize = .zero
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = false
      textView.autoresizingMask = [.width]
      textView.textContainer?.widthTracksTextView = true
      textView.textContainer?.lineFragmentPadding = 0
    }
  }
}

private enum WritingPresentationStyle {
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

extension Collection {
  fileprivate subscript(safe index: Index) -> Element? {
    indices.contains(index) ? self[index] : nil
  }
}
