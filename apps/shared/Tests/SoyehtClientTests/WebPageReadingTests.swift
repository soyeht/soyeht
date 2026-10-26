#if os(macOS)
  import AppKit
  import Foundation
  import SoyehtProtocol
  import Testing
  import WebKit

  @testable import SoyehtClient

  @MainActor private func hue(of image: NSImage, x: Double, y: Double) -> Double? {
    guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
      return nil
    }
    let bitmap = NSBitmapImageRep(cgImage: cgImage)
    let scale = Double(bitmap.pixelsWide) / image.size.width
    guard let color = bitmap.colorAt(x: Int(x * scale), y: Int(y * scale)) else { return nil }
    return Double(color.hueComponent) * 360
  }

  private func hueDistance(_ left: Double, _ right: Double) -> Double {
    let difference = abs(left - right).truncatingRemainder(dividingBy: 360)
    return min(difference, 360 - difference)
  }

  @Test @MainActor func aReadStitchesEveryScreenInPlaceAndStopsAtTheEndOfThePage() async throws {
    let fixture = await WebPageFixture.loaded()

    let capture = try await fixture.read(screens: ProtocolLimits.webReadScreens)

    let pixels = capture.image.size.width * capture.image.size.height
    let png = capture.image.tiffRepresentation.flatMap { NSBitmapImageRep(data: $0) }?
      .representation(using: .png, properties: [:])

    #expect(pixels <= WebPageReader.pixelBudget)
    #expect((png?.count ?? .max) <= ProtocolLimits.webScreenshotBytes)
    #expect(capture.endOfPage)
    #expect(capture.screens > 1 && capture.screens <= ProtocolLimits.webReadScreens)
    // Each 300 px block sits where the page put it, below the 40 px bar, whichever screen it was taken on.
    for block in 0..<10 {
      let sampled = hue(of: capture.image, x: 700, y: Double(40 + block * 300 + 150))
      let expected = Double(WebTestPages.hue(block))
      #expect(
        sampled.map { hueDistance($0, expected) < 8 } == true,
        "block \(block): \(String(describing: sampled)) vs \(expected)")
    }
    let page = try await fixture.webView.callAsyncJavaScript(
      "return document.documentElement.scrollHeight", arguments: [:], in: nil,
      contentWorld: WebPageWorld.isolated)
    #expect(abs(capture.image.size.height / capture.image.size.width - 3_040.0 / 800) < 0.02)
    #expect((page as? Double) == 3_040)
  }

  @Test @MainActor func aReadReturnsTheVisibleTextAndLinksOnceEach() async throws {
    let fixture = await WebPageFixture.loaded()

    let capture = try await fixture.read(screens: ProtocolLimits.webReadScreens)

    let lines = capture.text.split(separator: "\n").map(String.init)
    #expect(lines.filter { $0 == "Sticky bar" }.count == 1)
    for block in 0..<10 { #expect(lines.filter { $0 == "Block \(block) text" }.count == 1) }
    #expect(
      capture.links.map(\.href) == [
        "https://reader.example/post/0", "https://reader.example/post/4",
        "https://reader.example/post/9",
      ])
    #expect(capture.links.first?.text == "Open post 0")
    #expect(!capture.truncated)
  }

  @Test @MainActor func aReadOfAnEndlessPageVisitsOnlyTheScreensItWasGiven() async throws {
    let fixture = WebPageFixture()
    await fixture.load(WebTestPages.endless, baseURL: "https://feed.example/")

    let capture = try await fixture.read(screens: 3)

    #expect(capture.screens == 3)
    #expect(!capture.endOfPage)
    #expect(capture.text.contains("Post 0"))
    #expect(capture.text.contains("Post 4"))
    let over = try await fixture.read(screens: ProtocolLimits.webReadScreens + 50)
    #expect(over.screens <= ProtocolLimits.webReadScreens)
  }

  @Test @MainActor func aReadStopsWhenTheCallerSaysAPersonTookOver() async throws {
    let fixture = await WebPageFixture.loaded()

    let failure = await fixture.read(failingAtCall: 3) {
      throw ClientFailure.webHost(.human_took_over, "taken")
    }

    #expect((failure as? ClientFailure)?.localizedDescription == "taken")
  }

  @Test @MainActor func aReadWithNoContentStillReturnsThePicture() async throws {
    let fixture = await WebPageFixture.loaded(WebTestPages.tall(blocks: 1))

    let capture = try await fixture.read(screens: 4, collectsContent: false)

    #expect(capture.screens == 1)
    #expect(capture.endOfPage)
    #expect(capture.text.isEmpty && capture.links.isEmpty)
  }

  @MainActor private func scrollOffset(_ fixture: WebPageFixture) async throws -> Double {
    let raw = try await fixture.webView.callAsyncJavaScript(
      "return window.scrollY", arguments: [:], in: nil, contentWorld: WebPageWorld.isolated)
    return (raw as? Double) ?? -1
  }

  @MainActor private func scroll(_ fixture: WebPageFixture, to offset: Double) async throws {
    _ = try await fixture.webView.callAsyncJavaScript(
      "window.scrollTo(0, \(offset))", arguments: [:], in: nil,
      contentWorld: WebPageWorld.isolated)
    try await Task.sleep(for: .milliseconds(400))
  }

  private func brightness(of image: NSImage, x: Double, y: Double) -> Double? {
    guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil),
      let color = NSBitmapImageRep(cgImage: cgImage).colorAt(x: Int(x), y: Int(y))
    else { return nil }
    return Double(color.brightnessComponent)
  }

  @Test @MainActor func aReadPutsThePersonsPlaceBackWhenItEnds() async throws {
    let fixture = await WebPageFixture.loaded()
    try await scroll(fixture, to: 900)

    let capture = try await fixture.read(screens: 3)

    #expect(capture.screens == 3)
    #expect(abs(try await scrollOffset(fixture) - 900) < 1)
  }

  @Test @MainActor func aReadThatFailsPutsThePlaceBackAndOneThatIsTakenOverDoesNot() async throws {
    let fixture = await WebPageFixture.loaded()
    try await scroll(fixture, to: 900)

    let resized = await fixture.read(failingAtCall: 4) {
      fixture.webView.setFrameSize(NSSize(width: 700, height: 500))
    }
    #expect((resized as? ClientFailure)?.localizedDescription.contains("resized") == true)
    #expect(abs(try await scrollOffset(fixture) - 900) < 1)

    fixture.webView.setFrameSize(NSSize(width: 800, height: 600))
    let taken = await fixture.read(failingAtCall: 5) {
      throw ClientFailure.webHost(.human_took_over, "taken")
    }
    #expect(taken != nil)
    #expect(abs(try await scrollOffset(fixture) - 900) > 1)
  }

  @Test @MainActor func aTakeoverIsCheckedRightBeforeTheFirstScrollDown() async throws {
    let fixture = await WebPageFixture.loaded()

    // Checks run 1: before going to the top, 2: before the first screen, 3: before scrolling down.
    let taken = await fixture.read(failingAtCall: 3) {
      throw ClientFailure.webHost(.human_took_over, "taken")
    }

    #expect(taken != nil)
    #expect(try await scrollOffset(fixture) == 0)
  }

  @Test @MainActor func aStickyBarTallerThanTheNativeJointLeavesNoBandInThePicture() async throws {
    let fixture = await WebPageFixture.loaded(WebTestPages.tall(barHeight: 90))

    let capture = try await fixture.read(screens: ProtocolLimits.webReadScreens)

    #expect(capture.endOfPage)
    #expect(abs(capture.image.size.height - 3_090) < 2)
    var dark: [Int] = []
    for row in stride(from: 100, to: Int(capture.image.size.height) - 1, by: 5) {
      if (brightness(of: capture.image, x: 700, y: Double(row)) ?? 0) < 0.5 { dark.append(row) }
    }
    #expect(dark.isEmpty, "dark rows at \(dark)")
  }

  @Test @MainActor func aReadScrollsTheInnerScrollerWhenTheDocumentDoesNotScroll() async throws {
    let fixture = await WebPageFixture.loaded(WebTestPages.innerScroller())

    let capture = try await fixture.read(screens: ProtocolLimits.webReadScreens)

    #expect(capture.endOfPage)
    #expect(capture.screens > 1)
    #expect(capture.text.contains("Row 0 text") && capture.text.contains("Row 9 text"))
    for row in 0..<10 {
      let sampled = hue(of: capture.image, x: 700, y: Double(row * 300 + 150))
      #expect(
        sampled.map { hueDistance($0, Double(WebTestPages.hue(row))) < 8 } == true, "row \(row)")
    }
    let limited = try await fixture.read(screens: 2)
    #expect(limited.screens == 2)
    #expect(!limited.endOfPage)
  }

  @Test @MainActor func aReadOfAPageLongerThanTheScreenLimitSaysItDidNotReachTheEnd() async throws {
    let fixture = await WebPageFixture.loaded(WebTestPages.tall(blocks: 80))

    let capture = try await fixture.read(
      screens: ProtocolLimits.webReadScreens, collectsContent: false)

    #expect(capture.screens == ProtocolLimits.webReadScreens)
    #expect(!capture.endOfPage)
  }
#endif
