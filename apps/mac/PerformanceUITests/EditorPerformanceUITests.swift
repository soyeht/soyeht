import AppKit
import Foundation
import XCTest

final class EditorPerformanceUITests: XCTestCase {
  @MainActor func testEditorWorkAndVisiblePages() throws {
    let report = try XCTUnwrap(
      ProcessInfo.processInfo.environment["SOYEHT_EDITOR_PERFORMANCE_REPORT"])
    let bundleIdentifier = try XCTUnwrap(
      ProcessInfo.processInfo.environment["SOYEHT_EDITOR_METER_APP_BUNDLE_ID"])
    let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
    app.launchArguments = ["--editor-performance-window-report", report]
    app.launch()
    defer { app.terminate() }
    let window = app.windows["editor-performance-fixture"]
    let start = window.buttons["start-editor-performance"]
    guard start.waitForExistence(timeout: 15) else {
      add(XCTAttachment(string: app.debugDescription))
      XCTFail("editor fixture absent")
      return
    }
    start.click()
    let next = window.buttons["next-editor-performance-scroll"]
    let ready = XCTNSPredicateExpectation(
      predicate: NSPredicate(format: "hittable == YES"), object: next)
    guard XCTWaiter.wait(for: [ready], timeout: 30) == .completed else {
      add(XCTAttachment(string: app.debugDescription))
      let error = window.staticTexts["editor-performance-error"]
      XCTFail("key phase did not finish: \(error.exists ? error.label : "no fixture error")")
      return
    }
    let pageStatus = window.staticTexts["editor-performance-page-status"]
    let editorScroll = window.scrollViews["editor-performance-scroll"]
    XCTAssertTrue(pageStatus.waitForExistence(timeout: 5))
    XCTAssertTrue(editorScroll.waitForExistence(timeout: 5))
    let failure = window.staticTexts["editor-performance-error"]
    var priorPages = pageCount(pageStatus.label)
    var validatedPages = 0
    for sample in 0..<48 {
      let delta: CGFloat = sample.isMultiple(of: 2) ? -1_000 : -36_000
      editorScroll.scroll(byDeltaX: 0, deltaY: delta)
      next.click()
      if failure.exists {
        XCTFail("editor fixture failed at scroll \(sample): \(failure.label)")
        return
      }
      let count = pageCount(pageStatus.label)
      if count > priorPages {
        XCTAssertEqual(count, priorPages + 1, "page count skipped")
        assertVisibleInkAndGutter(window.screenshot())
        let fromLine = loadedLine(pageStatus.label)
        let editorText = editorScroll.textViews.firstMatch.value as? String ?? ""
        XCTAssertTrue(
          editorText.contains(String(format: "// line %06d", fromLine)),
          "visible page content does not match its gutter range")
        validatedPages += 1
      }
      priorPages = count
    }
    XCTAssertGreaterThanOrEqual(validatedPages, 20)
    XCTAssertTrue(window.staticTexts["editor-performance-done"].waitForExistence(timeout: 3))
    let receipt = XCTAttachment(string: "validated real-screen pages: \(validatedPages)")
    receipt.lifetime = .keepAlways
    add(receipt)
  }

  private func pageCount(_ label: String) -> Int {
    Int(
      label.components(separatedBy: ";").first?
        .replacingOccurrences(of: "Pages ", with: "") ?? "") ?? -1
  }

  private func loadedLine(_ label: String) -> Int {
    Int(label.components(separatedBy: "fromLine ").last ?? "") ?? -1
  }

  @MainActor private func assertVisibleInkAndGutter(_ screenshot: XCUIScreenshot) {
    guard let bitmap = NSBitmapImageRep(data: screenshot.pngRepresentation) else {
      XCTFail("window screenshot could not be decoded")
      return
    }
    let scale = CGFloat(bitmap.pixelsWide) / 1000
    let gutter = (Int(8 * scale), Int(42 * scale))
    let text = (Int(60 * scale), Int(370 * scale))
    let backgroundX = min(bitmap.pixelsWide - 1, Int(550 * scale))
    let top = min(bitmap.pixelsHigh - 1, Int(65 * scale))
    let bottom = min(bitmap.pixelsHigh, Int(650 * scale))
    var gutterRows = Set<Int>()
    var textRows = Set<Int>()
    var textInk = 0
    for y in stride(from: top, to: bottom, by: 2) {
      guard
        let background = bitmap.colorAt(x: backgroundX, y: y)?
          .usingColorSpace(.deviceRGB)
      else { continue }
      var gutterInk = 0
      var rowTextInk = 0
      for x in stride(from: gutter.0, to: min(bitmap.pixelsWide, gutter.1), by: 2) {
        if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
          distance(color, background) > 0.12
        {
          gutterInk += 1
        }
      }
      for x in stride(from: text.0, to: min(bitmap.pixelsWide, text.1), by: 2) {
        if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
          distance(color, background) > 0.18
        {
          rowTextInk += 1
        }
      }
      if gutterInk >= 2 { gutterRows.insert(y) }
      if rowTextInk >= 4 { textRows.insert(y) }
      textInk += rowTextInk
    }
    let aligned = gutterRows.filter { row in
      (-4...4).contains { textRows.contains(row + $0) }
    }.count
    XCTAssertGreaterThan(textInk, 500, "editor page has no text ink on screen")
    XCTAssertGreaterThan(aligned, 20, "gutter numbers do not align with visible text")
  }

  private func distance(_ lhs: NSColor, _ rhs: NSColor) -> CGFloat {
    abs(lhs.redComponent - rhs.redComponent)
      + abs(lhs.greenComponent - rhs.greenComponent)
      + abs(lhs.blueComponent - rhs.blueComponent)
  }
}
