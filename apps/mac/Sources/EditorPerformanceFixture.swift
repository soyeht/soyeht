#if SOYEHT_PERFORMANCE_METER
  import AppKit
  import SoyehtClient
  import SoyehtProtocol
  import SwiftUI
  import os

  @MainActor enum EditorPerformanceFixture {
    static let signpostSubsystem = "com.soyeht.next.dev.mac"
    static let signpostCategory = "EditorPerformance"
    static let waitSignpostName: StaticString = "EditorKeyToDrawStart"
    static let drawSignpostName: StaticString = "EditorDraw"
    static let processingSignpostName: StaticString = "EditorKeyProcessing"
    static let insertSignpostName: StaticString = "EditorInsertText"
    static let keyToInsertSignpostName: StaticString = "EditorKeyToInsertText"
    static let scrollSignpostName: StaticString = "EditorScrollProcessing"
    static let pageLoadSignpostName: StaticString = "EditorPageLoad"
    static let scrollDrawSignpostName: StaticString = "EditorScrollDraw"
    private static let viewportSize = NSSize(width: 1000, height: 720)
    private static let sampleCount = 240
    private static let scrollSampleCount = 48
    private static let fileLineCount: UInt32 = 100_000
    private static var current: Session?

    struct Options {
      let report: URL
    }

    private final class PerformanceWindow: NSWindow {
      var onScrollStart: (() -> Void)?
      var onScrollEnd: (() -> Void)?
      var onScrollDiagnostic: ((NSEvent, NSView?, String) -> Void)?

      override func sendEvent(_ event: NSEvent) {
        guard event.type == .scrollWheel else {
          super.sendEvent(event)
          return
        }
        let hitView = contentView?.hitTest(event.locationInWindow)
        onScrollDiagnostic?(event, hitView, "before")
        onScrollStart?()
        super.sendEvent(event)
        onScrollEnd?()
        onScrollDiagnostic?(event, hitView, "after")
      }
    }

    static func requestedOptions() -> Options? {
      let arguments = CommandLine.arguments
      guard let flag = arguments.firstIndex(of: "--editor-performance-window-report"),
        arguments.indices.contains(flag + 1)
      else { return nil }
      return Options(report: URL(fileURLWithPath: arguments[flag + 1]))
    }

    static func launch(_ options: Options) {
      current = Session(options: options)
      current?.start()
    }

    private enum Failure: Error {
      case keyEvent
      case missingKeyDraw
      case missingKeyProcessing(processing: Int, draw: Int, key: Int)
      case missingTextEdit
      case invalidFocus
      case missingScrollEvent(sample: Int)
      case missingScrollMovement(sample: Int, before: CGFloat, after: CGFloat)
      case invalidSampleEvents(sample: Int)
      case finalTextMismatch
      case invalidScrollGeometry(
        documentHeight: CGFloat, viewportHeight: CGFloat,
        documentMatches: Bool, windowSize: NSSize)
      case missingScrollDraw
      case missingScrollWork
      case missingPageLoad
    }

    @MainActor private final class Session {
      private let options: Options
      private let window: PerformanceWindow
      private let scrollView: NSScrollView
      private let textView: EditorTextView
      private let host: EditorViewportHostView
      private let coordinator: EditorNativeViewport.Coordinator
      private var editorView: EditorFileView
      private let initialText: String
      private let insertionOffset: Int
      private let signposter = OSSignposter(
        subsystem: signpostSubsystem, category: signpostCategory)
      private var waitInterval: OSSignpostIntervalState?
      private var drawInterval: OSSignpostIntervalState?
      private var processingInterval: OSSignpostIntervalState?
      private var keyToInsertInterval: OSSignpostIntervalState?
      private var eventStart: ContinuousClock.Instant?
      private var drawStart: ContinuousClock.Instant?
      private var processingStart: ContinuousClock.Instant?
      private var keyDownStart: ContinuousClock.Instant?
      private var activeSample: Int?
      private var activeSignpostID: OSSignpostID?
      private enum SampleEvent: Equatable { case keyDown, insertText, draw }
      private var sampleEvents = [[SampleEvent]](repeating: [], count: sampleCount)
      private var keyDurations: [Double] = []
      private var waitDurations: [Double] = []
      private var drawDurations: [Double] = []
      private var processingDurations: [Double] = []
      private var keyToInsertDurations: [Double] = []
      private var workDurations: [Double] = []
      private var scrollDispatchDurations: [Double] = []
      private var scrollDrawDurations: [Double] = []
      private var pageLoadDurations: [Double] = []
      private var scrollWorkDurations: [Double] = []
      private var scrollIsActive = false
      private var scrollStartOffset: CGFloat?
      private var scrollPageLoadsBefore = 0
      private var currentScrollDispatchDuration = 0.0
      private var scrollDispatchStart: ContinuousClock.Instant?
      private var scrollDispatchInterval: OSSignpostIntervalState?
      private var scrollDrawStart: ContinuousClock.Instant?
      private var scrollDrawInterval: OSSignpostIntervalState?
      private var currentScrollDrawDuration = 0.0
      private var dirtyRectAreas: [Double] = []
      private var statusLabel: NSTextField?
      private var pageStatusLabel: NSTextField?
      private var scrollButton: NSButton?
      private var scrollSample = 0

      init(options: Options) {
        self.options = options
        let (source, editorView) = Self.makeEditorView()
        let editor = EditorNativeViewport(
          client: EngineClient(socket: ""), view: editorView,
          selectedPosition: .constant(nil), navigationTarget: nil, diffMarkers: [:],
          agentReviewLines: [], agentCursors: [], vimEnabled: false,
          vimMode: .constant(.normal), vimCommandLine: .constant(nil),
          onDefinition: { _ in }, onReferences: { _ in })
        let coordinator = editor.makeCoordinator()
        let host = coordinator.makeHostView()
        host.frame = NSRect(origin: .zero, size: viewportSize)
        let textView = coordinator.performanceTextView
        let scrollView = host.scrollView
        scrollView.setAccessibilityIdentifier("editor-performance-scroll")
        scrollView.verticalScroller?.setAccessibilityIdentifier(
          "editor-performance-vertical-scrollbar")
        let window = PerformanceWindow(
          contentRect: NSRect(origin: NSPoint(x: 80, y: 80), size: viewportSize),
          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Editor Performance Fixture"
        window.setAccessibilityIdentifier("editor-performance-fixture")
        window.contentView = host
        self.textView = textView
        self.initialText = source
        self.insertionOffset = (source as NSString).range(of: "\n").location
        self.scrollView = scrollView
        self.host = host
        self.coordinator = coordinator
        self.editorView = editorView
        self.window = window
        textView.onPerformanceKeyDownStart = { [weak self] in self?.beginKeyProcessing() }
        textView.onPerformanceKeyDownEnd = { [weak self] in self?.endKeyProcessing() }
        textView.onPerformanceInsertText = { [weak self] in self?.recordInsertText() }
        textView.onPerformanceDrawStart = { [weak self] _ in self?.beginDraw() }
        textView.onPerformanceDraw = { [weak self] rect in self?.finishDraw(dirtyRect: rect) }
        textView.onPerformanceScrollWheel = { [weak self, weak textView] event, stage in
          self?.recordScrollDiagnostic(event, hitView: textView, stage: stage)
        }
        coordinator.onPerformanceViewportRequested = { [weak self] start in
          self?.loadViewport(fromLine: start)
        }
        window.onScrollStart = { [weak self] in self?.beginScrollEvent() }
        window.onScrollEnd = { [weak self] in self?.endScrollEvent() }
        window.onScrollDiagnostic = { [weak self] event, hitView, stage in
          self?.recordScrollDiagnostic(event, hitView: hitView, stage: stage)
        }
      }

      func start() {
        let container = NSView(frame: NSRect(origin: .zero, size: viewportSize))
        host.frame = container.bounds
        host.autoresizingMask = [.width, .height]
        container.addSubview(host)
        let button = NSButton(
          title: "Start Measurement", target: self, action: #selector(startMeasurement(_:)))
        button.setAccessibilityIdentifier("start-editor-performance")
        button.frame = NSRect(
          x: viewportSize.width - 190, y: viewportSize.height - 44,
          width: 180, height: 32)
        container.addSubview(button)
        let status = NSTextField(labelWithString: "Measurement Complete")
        status.setAccessibilityIdentifier("editor-performance-done")
        status.frame = NSRect(
          x: viewportSize.width - 190, y: viewportSize.height - 76,
          width: 180, height: 22)
        status.isHidden = true
        container.addSubview(status)
        statusLabel = status
        let pageStatus = NSTextField(labelWithString: "Pages 0; fromLine 0")
        pageStatus.setAccessibilityIdentifier("editor-performance-page-status")
        pageStatus.frame = NSRect(
          x: viewportSize.width - 270, y: viewportSize.height - 150,
          width: 260, height: 22)
        container.addSubview(pageStatus)
        pageStatusLabel = pageStatus
        let next = NSButton(
          title: "Next scroll", target: self, action: #selector(nextScroll(_:)))
        next.setAccessibilityIdentifier("next-editor-performance-scroll")
        next.frame = NSRect(
          x: viewportSize.width - 190, y: viewportSize.height - 112,
          width: 180, height: 32)
        next.isHidden = true
        container.addSubview(next)
        scrollButton = next
        window.contentView = container
        window.contentMinSize = EditorPerformanceFixture.viewportSize
        window.contentMaxSize = EditorPerformanceFixture.viewportSize
        window.setContentSize(EditorPerformanceFixture.viewportSize)
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(textView)
        coordinator.update(editorView)
        host.layoutSubtreeIfNeeded()
        if let container = textView.textContainer {
          textView.layoutManager?.ensureLayout(for: container)
        }
        window.displayIfNeeded()
      }

      @objc private func startMeasurement(_ sender: NSButton) {
        sender.isEnabled = false
        window.makeKeyAndOrderFront(nil)
        DispatchQueue.main.async { [weak self] in
          guard let self else { return }
          do {
            try self.measureKeys()
            self.updatePageStatus()
            self.scrollButton?.isHidden = false
          } catch {
            self.recordFailure(error)
          }
        }
      }

      @objc private func nextScroll(_ sender: NSButton) {
        do {
          try finishScrollSample()
          scrollSample += 1
          sender.title = "Scroll \(scrollSample); pages \(pageLoadDurations.count)"
          updatePageStatus()
          if scrollSample == EditorPerformanceFixture.scrollSampleCount {
            sender.isHidden = true
            try finishMeasurement()
            statusLabel?.isHidden = false
          }
        } catch {
          recordFailure(error)
        }
      }

      private func loadViewport(fromLine start: UInt32) {
        let begin = ContinuousClock.now
        let interval = signposter.beginInterval(pageLoadSignpostName)
        let visibleOrigin = scrollView.contentView.bounds.origin
        let count = UInt32(ProtocolLimits.editorViewportLines)
        let end = min(start + count, EditorPerformanceFixture.fileLineCount)
        let lines = (start..<end).map {
          EditorLine(line: $0, text: Self.lineText($0), highlights: [])
        }
        let viewport = FileReadResult(
          file: editorView.file, source: .buffer, fromLine: start, toLine: end,
          lines: lines)
        editorView = EditorFileView(file: editorView.file, viewport: viewport)
        coordinator.parent.view = editorView
        coordinator.isPerformanceLoadingViewport = true
        coordinator.update(editorView)
        scrollView.contentView.scroll(to: visibleOrigin)
        scrollView.reflectScrolledClipView(scrollView.contentView)
        coordinator.isPerformanceLoadingViewport = false
        signposter.endInterval(pageLoadSignpostName, interval)
        pageLoadDurations.append(Self.milliseconds(since: begin))
      }

      private static func makeEditorView() -> (String, EditorFileView) {
        let line = lineText(0)
        let texts = (0..<ProtocolLimits.editorViewportLines).map {
          lineText(UInt32($0))
        }
        let source = texts.joined(separator: "\n")
        let loadedLineCount = UInt32(texts.count)
        let file = FileDocument(
          fileId: "editor-performance-file", paneId: "editor-performance-pane",
          workspaceId: "editor-performance-workspace", path: "fixture.swift",
          language: .swift, revision: "1", diskRevision: "1", lineCount: fileLineCount,
          byteCount: String((line.utf8.count + 1) * Int(fileLineCount)), dirty: false,
          conflicted: false,
          watching: false)
        let viewport = FileReadResult(
          file: file, source: .buffer, fromLine: 0, toLine: loadedLineCount,
          lines: texts.enumerated().map {
            EditorLine(line: UInt32($0.offset), text: $0.element, highlights: [])
          })
        return (source, EditorFileView(file: file, viewport: viewport))
      }

      private static func lineText(_ line: UInt32) -> String {
        String(format: "// line %06d", line)
      }

      private func updatePageStatus() {
        let offset = Int(scrollView.contentView.bounds.minY.rounded())
        pageStatusLabel?.stringValue =
          "Pages \(pageLoadDurations.count); fromLine \(editorView.viewport?.fromLine ?? 0); offsetPt \(offset)"
      }

      private func finishMeasurement() throws {
        guard pageLoadDurations.count >= 20 else { throw Failure.missingPageLoad }
        guard scrollDrawDurations.count == EditorPerformanceFixture.scrollSampleCount
        else { throw Failure.missingScrollDraw }
        guard scrollWorkDurations.count >= 20 else { throw Failure.missingScrollWork }
        let report: [String: Any] = [
          "keystroke_to_pixel_p95_ms": Self.percentile95(keyDurations),
          "key_to_draw_start_median_ms": Self.median(waitDurations),
          "key_to_draw_start_p95_ms": Self.percentile95(waitDurations),
          "draw_median_ms": Self.median(drawDurations),
          "draw_p95_ms": Self.percentile95(drawDurations),
          "key_processing_median_ms": Self.median(processingDurations),
          "key_processing_p95_ms": Self.percentile95(processingDurations),
          "key_to_insert_median_ms": Self.median(keyToInsertDurations),
          "key_to_insert_p95_ms": Self.percentile95(keyToInsertDurations),
          "key_work_median_ms": Self.median(workDurations),
          "key_work_p95_ms": Self.percentile95(workDurations),
          "scroll_event_dispatch_p95_ms": Self.percentile95(scrollDispatchDurations),
          "scroll_draw_p95_ms": Self.percentile95(scrollDrawDurations),
          "page_load_p95_ms": Self.percentile95(pageLoadDurations),
          "page_load_samples": pageLoadDurations.count,
          "scroll_work_p95_ms": Self.percentile95(scrollWorkDurations),
          "scroll_work_samples": scrollWorkDurations.count,
          "key_work_samples": workDurations.count,
          "key_event_samples": keyDurations.count,
          "validated_key_samples": sampleEvents.filter {
            $0 == [.keyDown, .insertText, .draw]
          }.count,
          "scroll_events": scrollDispatchDurations.count,
          "scroll_offset_pt": scrollView.contentView.bounds.minY,
          "file_line_count": EditorPerformanceFixture.fileLineCount,
          "loaded_viewport_lines": ProtocolLimits.editorViewportLines,
          "document_height_pt": textView.frame.height,
          "viewport_height_pt": scrollView.contentSize.height,
          "dirty_rect_area_p95": Self.percentile95(dirtyRectAreas),
          "visual_validation": "pending_ui_screenshot",
        ]
        let data = try JSONSerialization.data(
          withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: options.report, options: .atomic)
      }

      private func recordFailure(_ error: Error) {
        FileHandle.standardError.write(Data("editor performance: \(error)\n".utf8))
        scrollButton?.isEnabled = false
        statusLabel?.setAccessibilityIdentifier("editor-performance-error")
        statusLabel?.stringValue = "Measurement failed: \(error)"
        statusLabel?.isHidden = false
        let partial: [String: Any] = [
          "page_load_samples": pageLoadDurations.count,
          "scroll_work_samples": scrollWorkDurations.count,
          "scroll_offset_pt": scrollView.contentView.bounds.minY,
        ]
        if let data = try? JSONSerialization.data(withJSONObject: partial) {
          try? data.write(to: options.report.appendingPathExtension("partial"), options: .atomic)
        }
        if let data = String(describing: error).data(using: .utf8) {
          try? data.write(to: options.report.appendingPathExtension("error"), options: .atomic)
        }
      }

      private func measureKeys() throws {
        guard window.isKeyWindow, window.makeFirstResponder(textView),
          window.firstResponder === textView
        else { throw Failure.invalidFocus }
        textView.setSelectedRange(NSRange(location: insertionOffset, length: 0))
        window.displayIfNeeded()
        for sample in 0..<sampleCount {
          try sendKey(sample: sample, value: Self.character(for: sample))
        }
        let original = initialText as NSString
        let typed = (0..<sampleCount).map { Self.character(for: $0) }.joined()
        let expected =
          original.substring(to: insertionOffset) + typed
          + original.substring(from: insertionOffset)
        guard textView.string == expected else { throw Failure.finalTextMismatch }
        let documentHeight = scrollView.documentView?.frame.height ?? 0
        let viewportHeight = scrollView.contentSize.height
        let documentMatches = scrollView.documentView === textView
        let windowSize = window.contentView?.bounds.size ?? .zero
        guard documentMatches, documentHeight > viewportHeight,
          abs(windowSize.width - EditorPerformanceFixture.viewportSize.width) < 1,
          abs(windowSize.height - EditorPerformanceFixture.viewportSize.height) < 1
        else {
          throw Failure.invalidScrollGeometry(
            documentHeight: documentHeight, viewportHeight: viewportHeight,
            documentMatches: documentMatches, windowSize: windowSize)
        }
        let loadedQuarter =
          CGFloat(ProtocolLimits.editorViewportLines / 4)
          * EditorTextMetrics.lineHeight
        scrollView.contentView.scroll(to: NSPoint(x: 0, y: loadedQuarter))
        scrollView.reflectScrolledClipView(scrollView.contentView)
        window.displayIfNeeded()
      }

      private static func character(for sample: Int) -> String {
        sample.isMultiple(of: 2) ? "x" : "y"
      }

      private func sendKey(sample: Int, value: String) throws {
        let before = textView.string
        guard
          let event = NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, characters: value,
            charactersIgnoringModifiers: value, isARepeat: false,
            keyCode: value == "x" ? 7 : 16)
        else { throw Failure.keyEvent }
        try sendMeasuredKey(event, sample: sample)
        guard textView.string.utf16.count == before.utf16.count + 1
        else { throw Failure.missingTextEdit }
      }

      private func beginScrollEvent() {
        if !scrollIsActive {
          scrollIsActive = true
          scrollStartOffset = scrollView.contentView.bounds.minY
          scrollPageLoadsBefore = pageLoadDurations.count
          currentScrollDispatchDuration = 0
          currentScrollDrawDuration = 0
        }
        scrollDispatchStart = .now
        scrollDispatchInterval = signposter.beginInterval(scrollSignpostName)
      }

      private func recordScrollDiagnostic(_ event: NSEvent, hitView: NSView?, stage: String) {
        let clip = scrollView.contentView
        let documentHeight = scrollView.documentView?.frame.height ?? -1
        var hitViews: [String] = []
        var view = hitView
        while let current = view {
          hitViews.append(String(describing: type(of: current)))
          view = current.superview
        }
        let hitPath = hitViews.joined(separator: ">")
        let line =
          "scroll diagnostic \(stage): deltaY=\(event.scrollingDeltaY) "
          + "precise=\(event.hasPreciseScrollingDeltas) phase=\(event.phase.rawValue) "
          + "momentum=\(event.momentumPhase.rawValue) window=\(event.windowNumber) "
          + "point=\(event.locationInWindow) hit=\(hitPath) "
          + "clipY=\(clip.bounds.minY) clipHeight=\(clip.bounds.height) "
          + "documentHeight=\(documentHeight) textHeight=\(textView.frame.height) "
          + "textMinHeight=\(textView.minSize.height)\n"
        FileHandle.standardError.write(Data(line.utf8))
      }

      private func endScrollEvent() {
        guard let scrollDispatchStart, let scrollDispatchInterval else { return }
        signposter.endInterval(scrollSignpostName, scrollDispatchInterval)
        currentScrollDispatchDuration += Self.milliseconds(since: scrollDispatchStart)
        self.scrollDispatchStart = nil
        self.scrollDispatchInterval = nil
      }

      private func finishScrollSample() throws {
        guard let before = scrollStartOffset else {
          throw Failure.missingScrollEvent(sample: scrollSample)
        }
        window.displayIfNeeded()
        RunLoop.current.run(until: Date().addingTimeInterval(0.018))
        window.displayIfNeeded()
        let after = scrollView.contentView.bounds.minY
        guard before != after else {
          throw Failure.missingScrollMovement(sample: scrollSample, before: before, after: after)
        }
        scrollDispatchDurations.append(currentScrollDispatchDuration)
        scrollDrawDurations.append(currentScrollDrawDuration)
        // The page swap is recorded separately for #404; ordinary scroll work
        // remains a gate even while that known layout hitch is unresolved.
        if pageLoadDurations.count == scrollPageLoadsBefore {
          scrollWorkDurations.append(currentScrollDispatchDuration + currentScrollDrawDuration)
        }
        scrollIsActive = false
        scrollStartOffset = nil
      }

      private func sendMeasuredKey(_ event: NSEvent, sample: Int) throws {
        let previousCount = keyDurations.count
        activeSample = sample
        let signpostID = signposter.makeSignpostID()
        activeSignpostID = signpostID
        eventStart = .now
        waitInterval = signposter.beginInterval(waitSignpostName, id: signpostID)
        window.sendEvent(event)
        let deadline = Date().addingTimeInterval(0.1)
        while keyDurations.count == previousCount, Date() < deadline {
          RunLoop.current.run(until: Date().addingTimeInterval(0.002))
          window.displayIfNeeded()
        }
        guard keyDurations.count != previousCount else { throw Failure.missingKeyDraw }
        guard sampleEvents[sample] == [.keyDown, .insertText, .draw]
        else { throw Failure.invalidSampleEvents(sample: sample) }
        guard processingDurations.count == keyDurations.count,
          keyToInsertDurations.count == keyDurations.count,
          drawDurations.count == keyDurations.count,
          let processing = processingDurations.last, let draw = drawDurations.last
        else {
          throw Failure.missingKeyProcessing(
            processing: processingDurations.count, draw: drawDurations.count,
            key: keyDurations.count)
        }
        workDurations.append(processing + draw)
        activeSample = nil
        activeSignpostID = nil
      }

      private func beginKeyProcessing() {
        guard let activeSample, let activeSignpostID else { return }
        sampleEvents[activeSample].append(.keyDown)
        processingStart = .now
        keyDownStart = processingStart
        processingInterval = signposter.beginInterval(
          processingSignpostName, id: activeSignpostID)
        keyToInsertInterval = signposter.beginInterval(
          keyToInsertSignpostName, id: activeSignpostID)
      }

      private func recordInsertText() {
        guard let activeSample, let activeSignpostID, let keyToInsertInterval,
          let keyDownStart
        else { return }
        sampleEvents[activeSample].append(.insertText)
        signposter.endInterval(keyToInsertSignpostName, keyToInsertInterval)
        self.keyToInsertInterval = nil
        self.keyDownStart = nil
        keyToInsertDurations.append(Self.milliseconds(since: keyDownStart))
        signposter.emitEvent(insertSignpostName, id: activeSignpostID)
      }

      private func endKeyProcessing() {
        guard let processingInterval, let processingStart else { return }
        signposter.endInterval(processingSignpostName, processingInterval)
        self.processingInterval = nil
        self.processingStart = nil
        processingDurations.append(Self.milliseconds(since: processingStart))
      }

      private func beginDraw() {
        if scrollIsActive {
          scrollDrawStart = .now
          scrollDrawInterval = signposter.beginInterval(scrollDrawSignpostName)
          return
        }
        guard let waitInterval, let eventStart, let activeSignpostID else { return }
        signposter.endInterval(waitSignpostName, waitInterval)
        self.waitInterval = nil
        waitDurations.append(Self.milliseconds(since: eventStart))
        drawStart = .now
        drawInterval = signposter.beginInterval(drawSignpostName, id: activeSignpostID)
      }

      private func finishDraw(dirtyRect: NSRect) {
        if scrollIsActive, let scrollDrawInterval, let scrollDrawStart {
          signposter.endInterval(scrollDrawSignpostName, scrollDrawInterval)
          self.scrollDrawInterval = nil
          self.scrollDrawStart = nil
          currentScrollDrawDuration += Self.milliseconds(since: scrollDrawStart)
          return
        }
        guard let drawInterval, let drawStart, let eventStart, let activeSample else { return }
        sampleEvents[activeSample].append(.draw)
        signposter.endInterval(drawSignpostName, drawInterval)
        self.drawInterval = nil
        self.drawStart = nil
        self.eventStart = nil
        drawDurations.append(Self.milliseconds(since: drawStart))
        let milliseconds = Self.milliseconds(since: eventStart)
        dirtyRectAreas.append(dirtyRect.width * dirtyRect.height)
        keyDurations.append(milliseconds)
      }

      private static func milliseconds(since start: ContinuousClock.Instant) -> Double {
        let duration = start.duration(to: .now)
        return Double(duration.components.seconds) * 1000
          + Double(duration.components.attoseconds) / 1_000_000_000_000_000
      }

      private static func percentile95(_ values: [Double]) -> Double {
        let ordered = values.sorted()
        let rank = Int((Double(ordered.count) * 0.95).rounded(.up))
        return ordered[min(ordered.count - 1, max(0, rank - 1))]
      }

      private static func median(_ values: [Double]) -> Double {
        let ordered = values.sorted()
        let middle = ordered.count / 2
        return ordered.count.isMultiple(of: 2)
          ? (ordered[middle - 1] + ordered[middle]) / 2 : ordered[middle]
      }
    }
  }

#endif
