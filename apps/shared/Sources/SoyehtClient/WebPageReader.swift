#if os(macOS)
  import AppKit
  import Foundation
  import SoyehtProtocol
  import WebKit

  /// What reading a page gave: one stitched picture and the words and links seen on its screens.
  public struct WebReadCapture {
    public var image: NSImage
    public var text: String
    public var links: [WebReadLink]
    public var screens: Int
    public var endOfPage: Bool
    public var truncated: Bool
  }

  /// Reads a web pane the way a person does (ADR 0109).
  ///
  /// A page that scrolls is scrolled with the view's own page-down action, so the page sees
  /// scroll events from the native scroller and nothing else. A page whose document does not
  /// scroll but whose app does (an inner element) is scrolled by that element, in the isolated
  /// world. The words and links are read from the document by a script in the isolated world,
  /// which dispatches no event and changes nothing. Pictures are the view's own snapshots, one
  /// per screen, joined by the offsets the page reported. The person's place is put back when the
  /// read ends, unless the person took over.
  @MainActor public final class WebPageReader {
    /// Pixel budget of the joined picture, so a long read does not allocate its full size.
    public static let pixelBudget = 4_000_000.0
    /// The share of a command's time a read may spend; the rest is left for the answer.
    public static let deadlineShare = 0.8
    /// Rows of one screen that the next screen repeats, so no row is lost at a joint.
    private static let jointOverlap = 8.0
    private let webView: WKWebView

    public init(webView: WKWebView) { self.webView = webView }

    public static func deadline(forTimeoutMs timeoutMs: UInt32) -> ContinuousClock.Instant {
      .now + .milliseconds(Int64(Double(timeoutMs) * deadlineShare))
    }

    /// - Parameters:
    ///   - screens: how many screens to visit at most, from the top of the page.
    ///   - collectsContent: whether to read the words and links as well as the pictures.
    ///   - deadline: after it, the read ends with the screens it has.
    ///   - between: runs before every scroll; it throws when a person took over, which stops the
    ///     read and leaves the page where the person has it.
    public func read(
      screens requested: Int, collectsContent: Bool, deadline: ContinuousClock.Instant,
      between: @escaping () throws -> Void
    ) async throws -> WebReadCapture {
      let origin = try await pageState(collect: false)
      let walk = Walk(
        limit: min(max(1, requested), ProtocolLimits.webReadScreens), collects: collectsContent,
        deadline: deadline, size: webView.bounds.size, zoom: webView.pageZoom, between: between)
      let capture: WebReadCapture
      do {
        capture = try await walk.run(self, inner: origin.inner)
      } catch let stop as Walk.PersonTookOver {
        throw stop.reason
      } catch {
        await restore(origin)
        throw error
      }
      if !walk.personIsAway() { await restore(origin) }
      return capture
    }

    /// Puts the person's place back. It runs after the capture is complete or has failed, so a
    /// failure to scroll must not replace what the caller gets.
    private func restore(_ origin: PageState) async {
      do {
        try await scroll(by: nil, to: origin.scrollY)
        try await settle()
      } catch {
        // The page went away or stopped answering; there is no place left to put back.
      }
    }

    /// One pass over the screens of a read.
    @MainActor final class Walk {
      struct PersonTookOver: Error { let reason: any Error }
      let limit: Int
      let collects: Bool
      let deadline: ContinuousClock.Instant
      let size: CGSize
      let zoom: Double
      let between: () throws -> Void
      var tiles: [WebReadTile] = []
      var images: [CGImage] = []
      var pointsWidth = 0.0

      init(
        limit: Int, collects: Bool, deadline: ContinuousClock.Instant, size: CGSize, zoom: Double,
        between: @escaping () throws -> Void
      ) {
        self.limit = limit
        self.collects = collects
        self.deadline = deadline
        self.size = size
        self.zoom = zoom
        self.between = between
      }

      func checkpoint() throws {
        do { try between() } catch { throw PersonTookOver(reason: error) }
      }

      func personIsAway() -> Bool {
        do {
          try between()
          return false
        } catch {
          return true
        }
      }

      func run(_ reader: WebPageReader, inner: Bool) async throws -> WebReadCapture {
        var accumulator = WebReadAccumulator()
        var endOfPage = false
        var previousOffset = -1.0
        try checkpoint()
        if inner {
          try await reader.scroll(by: nil, to: 0)
        } else {
          reader.webView.scrollToBeginningOfDocument(nil)
        }
        try await reader.settle()
        for index in 0..<limit {
          try checkpoint()
          let state = try await reader.pageState(collect: collects)
          try reader.requireSameSize(size, zoom)
          if index > 0, state.scrollY <= previousOffset + 0.5 { break }
          let tile = try await reader.tile(state.box, zoom: zoom)
          try reader.requireSameSize(size, zoom)
          images.append(tile.image)
          pointsWidth = pointsWidth == 0 ? tile.points.width : pointsWidth
          tiles.append(
            WebReadTile(
              offset: state.scrollY * zoom, height: tile.points.height,
              topInset: state.topBand * zoom, bottomInset: state.bottomBand * zoom))
          accumulator.add(blocks: state.blocks, links: state.links)
          if state.cut { accumulator.markTruncated() }
          previousOffset = state.scrollY
          endOfPage = state.scrollY + state.viewHeight >= state.pageHeight - 1
          if endOfPage || index == limit - 1 || ContinuousClock.now >= deadline { break }
          try await advance(reader, from: state)
        }
        return WebReadCapture(
          image: try reader.stitched(images, tiles, width: pointsWidth), text: accumulator.text,
          links: accumulator.links, screens: images.count, endOfPage: endOfPage,
          truncated: accumulator.truncated)
      }

      /// One screen down. The view's own page-down leaves a joint of about 40 points; when the
      /// page keeps more than that fixed on screen, the extra is scrolled back, so that nothing
      /// that hid behind a sticky bar is lost.
      private func advance(_ reader: WebPageReader, from state: PageState) async throws {
        let step = max(
          1, state.viewHeight - state.topBand - state.bottomBand - WebPageReader.jointOverlap)
        try checkpoint()
        if state.inner {
          try await reader.scroll(by: step, to: nil)
          try await reader.settle()
          return
        }
        reader.webView.scrollPageDown(nil)
        try await reader.settle()
        let moved = try await reader.pageState(collect: false).scrollY - state.scrollY
        if moved > step {
          try checkpoint()
          try await reader.scroll(by: step - moved, to: nil)
          try await reader.settle()
        }
      }
    }

    private func scroll(by: Double?, to: Double?) async throws {
      _ = try await webView.callAsyncJavaScript(
        WebReadScripts.scroll,
        arguments: ["by": by ?? NSNull(), "to": to ?? NSNull()], in: nil,
        contentWorld: WebPageWorld.isolated)
    }

    private func requireSameSize(_ size: CGSize, _ zoom: Double) throws {
      guard webView.bounds.size == size, webView.pageZoom == zoom else {
        throw ClientFailure.webHost(.internal_error, "The web pane was resized during the read.")
      }
    }

    /// Waits for the animated scroll and for content the scroll loaded to stop moving.
    private func settle() async throws {
      var previous: PageState?
      var steady = 0
      for _ in 0..<20 {
        try await Task.sleep(for: .milliseconds(80))
        let state = try await pageState(collect: false)
        steady = state.sameGeometry(as: previous) ? steady + 1 : 0
        if steady >= 2 { return }
        previous = state
      }
    }

    private func pageState(collect: Bool) async throws -> PageState {
      let raw = try await webView.callAsyncJavaScript(
        WebReadScripts.state, arguments: ["collect": collect], in: nil,
        contentWorld: WebPageWorld.isolated)
      let failure = ClientFailure.webHost(.internal_error, "The page returned an invalid state.")
      guard let json = raw as? String, let data = json.data(using: .utf8) else { throw failure }
      do { return try JSONDecoder().decode(PageState.self, from: data) } catch { throw failure }
    }

    /// The view's own picture of `configuration.rect`, which is how a person's screen looks.
    public static func snapshot(
      _ webView: WKWebView, configuration: WKSnapshotConfiguration
    ) async throws -> NSImage {
      try await withCheckedThrowingContinuation { continuation in
        webView.takeSnapshot(with: configuration) { image, error in
          if let image {
            continuation.resume(returning: image)
          } else {
            continuation.resume(
              throwing: error ?? ClientFailure.webHost(.internal_error, "The page snapshot failed.")
            )
          }
        }
      }
    }

    /// The picture of the scroller's visible box (the whole view for a document that scrolls).
    private func tile(_ box: PageState.Box, zoom: Double) async throws -> (
      image: CGImage, points: CGSize
    ) {
      let rect = CGRect(
        x: box.x * zoom, y: box.y * zoom, width: box.width * zoom, height: box.height * zoom
      ).intersection(webView.bounds)
      guard !rect.isNull, rect.width > 0, rect.height > 0 else {
        throw ClientFailure.webHost(.internal_error, "The web viewport is unavailable.")
      }
      let configuration = WKSnapshotConfiguration()
      configuration.rect = rect
      let image = try await Self.snapshot(webView, configuration: configuration)
      guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        throw ClientFailure.webHost(.internal_error, "The page snapshot could not be read.")
      }
      return (cgImage, image.size)
    }

    private func stitched(_ images: [CGImage], _ tiles: [WebReadTile], width: Double) throws
      -> NSImage
    {
      let layout = WebReadStitch.strips(tiles)
      guard let first = images.first, width > 0, layout.height > 0 else {
        throw ClientFailure.webHost(.internal_error, "The web viewport is unavailable.")
      }
      let scale = WebReadStitch.outputScale(
        width: width, height: layout.height, nativeScale: Double(first.width) / width,
        pixelBudget: Self.pixelBudget)
      let pixelsWide = max(1, Int((width * scale).rounded()))
      let pixelsHigh = max(1, Int((layout.height * scale).rounded()))
      guard
        let context = CGContext(
          data: nil, width: pixelsWide, height: pixelsHigh, bitsPerComponent: 8, bytesPerRow: 0,
          space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
          bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue)
      else {
        throw ClientFailure.webHost(.internal_error, "The page snapshot could not be joined.")
      }
      context.interpolationQuality = .high
      for strip in layout.strips {
        let source = images[strip.tile]
        let nativeScale = Double(source.width) / width
        let crop = CGRect(
          x: 0, y: (strip.sourceY * nativeScale).rounded(), width: Double(source.width),
          height: (strip.height * nativeScale).rounded())
        guard let piece = source.cropping(to: crop) else { continue }
        let top = (strip.destinationY * scale).rounded()
        let bottom = ((strip.destinationY + strip.height) * scale).rounded()
        context.draw(
          piece,
          in: CGRect(
            x: 0, y: Double(pixelsHigh) - bottom, width: Double(pixelsWide), height: bottom - top))
      }
      guard let joined = context.makeImage() else {
        throw ClientFailure.webHost(.internal_error, "The page snapshot could not be joined.")
      }
      return NSImage(cgImage: joined, size: NSSize(width: pixelsWide, height: pixelsHigh))
    }
  }

  struct PageState: Decodable {
    struct Box: Decodable {
      var x: Double
      var y: Double
      var width: Double
      var height: Double
    }
    var scrollY: Double
    var viewHeight: Double
    var pageHeight: Double
    var inner: Bool
    var box: Box
    var topBand: Double
    var bottomBand: Double
    var cut: Bool
    var blocks: [WebReadBlock]
    var links: [WebReadLink]

    func sameGeometry(as other: PageState?) -> Bool {
      guard let other else { return false }
      return abs(scrollY - other.scrollY) < 0.5 && abs(pageHeight - other.pageHeight) < 0.5
    }
  }
#endif
