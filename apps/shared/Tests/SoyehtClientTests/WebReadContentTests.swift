import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func developmentHostsAreObservedAndOtherSitesNever() {
  for local in [
    "http://localhost:5173/", "https://app.localhost/", "http://127.0.0.1:3000/",
    "http://[::1]:8080/", "http://192.168.1.20:3000/", "http://10.0.0.4/",
    "http://172.16.5.5/", "http://172.31.255.1/", "http://printer.local/",
  ] {
    #expect(WebObservationPolicy.observes(URL(string: local)), "\(local)")
  }
  for remote in [
    "https://x.com/home", "https://example.com/", "http://172.32.0.1/", "http://11.0.0.1/",
    "http://192.169.0.1/", "https://localhost.example.com/", "https://notlocal.test/",
    "file:///tmp/index.html", "about:blank",
    // Forms that read as private to a loose parser: octal octets, IPv6 other than ::1, a dot
    // after the name, and names that merely contain a local address.
    "http://010.0.0.1/", "http://172.016.0.1/", "http://127.1/", "http://[fd00::1]/",
    "http://[::ffff:127.0.0.1]/", "http://localhost./", "http://127.0.0.1.nip.io/",
    "http://0x7f.0.0.1/", "http://2130706433/", "http://100.64.0.1/",
  ] {
    #expect(!WebObservationPolicy.observes(URL(string: remote)), "\(remote)")
  }
  #expect(!WebObservationPolicy.observes(nil))
}

@Test func readTextKeepsOneCopyOfWhatScreensRepeat() {
  var accumulator = WebReadAccumulator()
  accumulator.add(
    blocks: [
      WebReadBlock(y: 0, text: "Site header", pinned: true),
      WebReadBlock(y: 80, text: "First post", pinned: false),
    ], links: [])
  accumulator.add(
    blocks: [
      WebReadBlock(y: 400, text: "Site header", pinned: true),
      WebReadBlock(y: 80, text: "First post", pinned: false),
      WebReadBlock(y: 700, text: "Second post", pinned: false),
      WebReadBlock(y: 900, text: "First post", pinned: false),
    ], links: [])

  #expect(accumulator.text == "Site header\nFirst post\nSecond post\nFirst post")
  #expect(!accumulator.truncated)
}

@Test func readTextAndLinksStayInsideTheirLimits() {
  var accumulator = WebReadAccumulator()
  let line = String(repeating: "w", count: 1_000)
  accumulator.add(
    blocks: (0..<100).map { WebReadBlock(y: $0, text: "\($0) \(line)", pinned: false) },
    links: (0..<150).map { WebReadLink(text: "link \($0)", href: "https://example.test/\($0)") })

  #expect(accumulator.text.utf8.count <= ProtocolLimits.webReadTextBytes)
  #expect(accumulator.links.count == ProtocolLimits.webReadLinks)
  #expect(accumulator.truncated)
}

@Test func readLinksAreBoundedOneByOne() {
  var accumulator = WebReadAccumulator()
  let tooLong =
    "https://example.test/" + String(repeating: "a", count: ProtocolLimits.webFieldBytes)
  accumulator.add(
    blocks: [],
    links: [
      WebReadLink(text: String(repeating: "t", count: 1_000), href: "https://example.test/ok"),
      WebReadLink(text: "dropped", href: tooLong),
      WebReadLink(text: "same", href: "https://example.test/ok"),
      WebReadLink(text: "same", href: "https://example.test/ok"),
    ])

  #expect(accumulator.links.count == 2)
  #expect(accumulator.links[0].text.utf8.count == ProtocolLimits.webTokenBytes)
  #expect(accumulator.truncated)
}

@Test func laterScreensAddOnlyTheRowsBelowWhatIsCovered() {
  let tiles = [
    WebReadTile(offset: 0, height: 600), WebReadTile(offset: 560, height: 600),
    WebReadTile(offset: 1_120, height: 600),
  ]
  let layout = WebReadStitch.strips(tiles)

  #expect(layout.height == 1_720)
  #expect(layout.strips.map(\.tile) == [0, 1, 2])
  #expect(layout.strips.map(\.sourceY) == [0, 40, 40])
  #expect(layout.strips.map(\.destinationY) == [0, 600, 1_160])
  #expect(layout.strips.map(\.height) == [600, 560, 560])
}

@Test func theLastScreenOfAShortPageContributesOnlyItsNewRows() {
  let layout = WebReadStitch.strips([
    WebReadTile(offset: 0, height: 600), WebReadTile(offset: 200, height: 600),
  ])

  #expect(layout.height == 800)
  #expect(layout.strips.last == WebReadStrip(tile: 1, sourceY: 400, destinationY: 600, height: 200))
}

@Test func theJoinedPictureStaysInsideThePixelBudget() {
  let scale = WebReadStitch.outputScale(
    width: 1_280, height: 9_600, nativeScale: 2, pixelBudget: 4_000_000)

  #expect(scale < 1)
  #expect(1_280 * scale * 9_600 * scale <= 4_000_001)
  #expect(
    WebReadStitch.outputScale(width: 400, height: 600, nativeScale: 2, pixelBudget: 4_000_000) == 2)
}

@Test func theReadValueDecodesAsTheContractResultWithoutThePaneIdentity() throws {
  let image = AttachmentRef(
    attachmentId: "00112233-4455-4677-8899-aabbccddeeff", mimeType: "image/png",
    name: "screenshot.png", bytes: "9")
  let value = try WebReadAssembly.value(
    text: "Hello", links: [WebReadLink(text: "Home", href: "https://example.test/")],
    screens: 3, endOfPage: true, truncated: false, generation: 7, title: "Feed",
    url: "https://example.test/", image: image)

  guard case .object(var fields) = value else {
    Issue.record("the read value is not an object")
    return
  }
  #expect(fields["paneId"] == nil)
  fields["paneId"] = .string("pane-1")
  let result = try JSONDecoder().decode(
    WebReadResult.self, from: JSONEncoder().encode(JsonValue.object(fields)))
  #expect(result.navigationGeneration == "7")
  #expect(result.screens == 3)
  #expect(result.links.first?.href == "https://example.test/")
  #expect(result.image.name == "screenshot.png")
}

@Test func aStickyBarIsKeptOnceAndNotAtEveryJoint() {
  let tiles = [
    WebReadTile(offset: 0, height: 600, topInset: 90),
    WebReadTile(offset: 510, height: 600, topInset: 90),
    WebReadTile(offset: 1_020, height: 600, topInset: 90),
  ]
  let layout = WebReadStitch.strips(tiles)

  #expect(layout.height == 1_620)
  #expect(layout.strips.map(\.sourceY) == [0, 90, 90])
  #expect(layout.strips.map(\.destinationY) == [0, 600, 1_110])
}

@Test func aFixedFooterIsKeptOnlyOnTheLastScreen() {
  let tiles = [
    WebReadTile(offset: 0, height: 600, bottomInset: 50),
    WebReadTile(offset: 540, height: 600, bottomInset: 50),
  ]
  let layout = WebReadStitch.strips(tiles)

  #expect(layout.strips.map(\.height) == [550, 600 - 10])
  #expect(layout.height == 1_140)
}

@Test func theNodeCapOfThePageScriptMarksTheReadTruncated() {
  var accumulator = WebReadAccumulator()
  accumulator.markTruncated()

  #expect(accumulator.truncated)
}

@Test func aResultThatEscapingWouldPushOverTheBudgetIsTrimmedNotRefused() throws {
  let image = AttachmentRef(
    attachmentId: "00112233-4455-4677-8899-aabbccddeeff", mimeType: "image/png",
    name: "screenshot.png", bytes: "9")
  let quotes = String(repeating: "\"", count: ProtocolLimits.webFieldBytes - 30)
  let links = (0..<ProtocolLimits.webReadLinks).map {
    WebReadLink(
      text: String(repeating: "\"", count: ProtocolLimits.webTokenBytes),
      href: "https://e.test/\($0)?q=\(quotes)")
  }
  let value = try WebReadAssembly.value(
    text: String(repeating: "line \"quoted\"\n", count: 4_000), links: links, screens: 3,
    endOfPage: false, truncated: false, generation: 1, title: "t", url: "https://e.test/",
    image: image)

  let size = try JSONEncoder().encode(value).count
  guard case .object(let fields) = value, case .bool(true)? = fields["truncated"] else {
    Issue.record("the trimmed result must say it is truncated")
    return
  }
  #expect(size + WebReadAssembly.envelopeBytes <= ProtocolLimits.webReadBytes)
}

@Test func aPageImageAsBase64AndCarriedTwiceFitsOneHarnessMessage() {
  // A harness drops its connection on a longer message, and may carry a tool result twice in it.
  let carried = (ProtocolLimits.webAgentImageBytes + 2) / 3 * 4 * 2
  #expect(carried < ProtocolLimits.acpMessageBytes)
  #expect(ProtocolLimits.webAgentImageBytes < ProtocolLimits.webScreenshotBytes)
}

// The cases the desktop client passes too (`contracts/vectors/web-read.json`): what a read keeps of
// each screen's text and links, how the screens are joined, and the scale of the joined picture.
private func webReadVector() throws -> [String: Any] {
  let url = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("contracts/vectors/web-read.json")
  return try #require(
    JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
}

private func decoded<T: Decodable>(_ type: T.Type, _ object: Any) throws -> T {
  try JSONDecoder().decode(type, from: JSONSerialization.data(withJSONObject: object))
}

@Test func theSharedVectorKeepsEachBlockAndLinkOnce() throws {
  let cases = try #require(try webReadVector()["accumulate"] as? [[String: Any]])
  #expect(!cases.isEmpty)
  for item in cases {
    let name = item["name"] as? String ?? ""
    var accumulator = WebReadAccumulator()
    for step in try #require(item["steps"] as? [[String: Any]]) {
      accumulator.add(
        blocks: try decoded([WebReadBlock].self, step["blocks"] ?? []),
        links: try decoded([WebReadLink].self, step["links"] ?? []))
    }
    let expect = try #require(item["expect"] as? [String: Any])
    #expect(accumulator.text == expect["text"] as? String, "\(name)")
    #expect(accumulator.links.count == expect["links"] as? Int, "\(name)")
    #expect(accumulator.truncated == expect["truncated"] as? Bool, "\(name)")
  }
}

@Test func theSharedVectorJoinsScreensByTheRowsBelowWhatIsCovered() throws {
  let cases = try #require(try webReadVector()["stitch"] as? [[String: Any]])
  #expect(!cases.isEmpty)
  for item in cases {
    let name = item["name"] as? String ?? ""
    let tiles = try #require(item["tiles"] as? [[String: Any]]).map { tile in
      WebReadTile(
        offset: (tile["offset"] as? Double) ?? 0, height: (tile["height"] as? Double) ?? 0,
        topInset: (tile["topInset"] as? Double) ?? 0,
        bottomInset: (tile["bottomInset"] as? Double) ?? 0)
    }
    let joined = WebReadStitch.strips(tiles)
    #expect(joined.height == item["height"] as? Double, "\(name)")
    let expected = try #require(item["strips"] as? [[String: Any]])
    #expect(joined.strips.count == expected.count, "\(name)")
    for (strip, want) in zip(joined.strips, expected) {
      #expect(strip.tile == want["tile"] as? Int, "\(name)")
      #expect(strip.sourceY == want["sourceY"] as? Double, "\(name)")
      #expect(strip.destinationY == want["destinationY"] as? Double, "\(name)")
      #expect(strip.height == want["height"] as? Double, "\(name)")
    }
  }
}

@Test func theSharedVectorScalesThePictureToTheBudget() throws {
  let cases = try #require(try webReadVector()["outputScale"] as? [[String: Any]])
  #expect(!cases.isEmpty)
  for item in cases {
    let scale = WebReadStitch.outputScale(
      width: try #require(item["width"] as? Double),
      height: try #require(item["height"] as? Double),
      nativeScale: try #require(item["nativeScale"] as? Double),
      pixelBudget: try #require(item["pixelBudget"] as? Double))
    #expect(abs(scale - (item["scale"] as? Double ?? .nan)) < 1e-9)
  }
}
