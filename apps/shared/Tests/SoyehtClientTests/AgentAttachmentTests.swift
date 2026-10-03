import CoreGraphics
import Foundation
import ImageIO
import SoyehtProtocol
import Testing
import UniformTypeIdentifiers

@testable import SoyehtClient

#if os(macOS)
  import AppKit
#endif

private func picture(width: Int, height: Int, as type: UTType, noise: Bool = false) throws -> Data {
  let context = try #require(
    CGContext(
      data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
      space: CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
  context.setFillColor(CGColor(red: 0.2, green: 0.4, blue: 0.6, alpha: 1))
  context.fill(CGRect(x: 0, y: 0, width: width, height: height))
  if noise {
    var generator = SystemRandomNumberGenerator()
    for _ in 0..<(width * height / 50) {
      context.setFillColor(
        CGColor(
          red: .random(in: 0...1, using: &generator), green: .random(in: 0...1, using: &generator),
          blue: .random(in: 0...1, using: &generator), alpha: 1))
      context.fill(
        CGRect(
          x: Int.random(in: 0..<width, using: &generator),
          y: Int.random(in: 0..<height, using: &generator), width: 6, height: 6))
    }
  }
  let image = try #require(context.makeImage())
  let output = NSMutableData()
  let destination = try #require(
    CGImageDestinationCreateWithData(output, type.identifier as CFString, 1, nil))
  CGImageDestinationAddImage(destination, image, nil)
  #expect(CGImageDestinationFinalize(destination))
  return output as Data
}

private func pixelSize(_ data: Data) throws -> Int {
  let source = try #require(CGImageSourceCreateWithData(data as CFData, nil))
  let properties = try #require(
    CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
  return max(
    properties[kCGImagePropertyPixelWidth] as? Int ?? 0,
    properties[kCGImagePropertyPixelHeight] as? Int ?? 0)
}

private func temporaryFile(_ name: String, contents: Data) throws -> URL {
  let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
    "attachments-\(UUID().uuidString)")
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  let url = directory.appendingPathComponent(name)
  try contents.write(to: url)
  return url
}

private func draft(bytes: Int, name: String = "file.bin") -> AgentAttachmentDraft {
  AgentAttachmentDraft(
    kind: .file, name: name, mimeType: "application/octet-stream", byteCount: bytes,
    source: .bytes(Data(count: bytes)))
}

@Test func attachmentListEnforcesCountFileAndTotalLimitsWithClearMessages() throws {
  var list = AgentAttachmentList()
  for _ in 0..<ProtocolLimits.promptAttachments {
    try list.add(
      AgentAttachmentDraft(
        kind: .file, name: "linked.pdf", mimeType: "application/pdf", byteCount: 50_000_000,
        source: .hostFile("/tmp/linked.pdf")))
  }
  #expect(throws: AgentAttachmentRejection.tooMany) { try list.add(draft(bytes: 1)) }
  #expect(AgentAttachmentRejection.tooMany.message.contains("20"))

  var sized = AgentAttachmentList()
  let big = draft(bytes: ProtocolLimits.promptAttachmentBytes + 1, name: "big.bin")
  #expect(throws: AgentAttachmentRejection.tooLarge(name: "big.bin")) { try sized.add(big) }
  #expect(
    AgentAttachmentRejection.tooLarge(name: "big.bin").message == "big.bin is larger than 10 MB.")
  try sized.add(draft(bytes: ProtocolLimits.promptAttachmentBytes))
  try sized.add(draft(bytes: ProtocolLimits.promptAttachmentBytes))
  #expect(throws: AgentAttachmentRejection.totalTooLarge) { try sized.add(draft(bytes: 1)) }
  #expect(throws: AgentAttachmentRejection.empty(name: "empty.bin")) {
    try sized.add(draft(bytes: 0, name: "empty.bin"))
  }
  let first = try #require(sized.items.first)
  sized.remove(first.id)
  #expect(sized.items.count == 1)
}

@Test func draftsDescribeUploadsWithFramesAndHostFilesByPath() throws {
  let upload = AgentAttachmentDraft(
    kind: .image, name: "shot.png", mimeType: "image/png", byteCount: 3,
    source: .bytes(Data([1, 2, 3])), thumbnail: Data([9]))
  let (attachment, frame) = upload.wire()
  let reference = try #require(frame?.reference)
  #expect(attachment.attachmentId == upload.id.uuidString.lowercased())
  #expect(reference.attachmentId == attachment.attachmentId)
  #expect(attachment.bytes == "3" && reference.bytes == "3")
  #expect(attachment.thumbnail == Data([9]).base64EncodedString())
  #expect(attachment.path == nil && attachment.data == nil)

  let linked = AgentAttachmentDraft(
    kind: .file, name: "report.pdf", mimeType: "application/pdf", byteCount: 2_048,
    source: .hostFile("/Users/example/report.pdf"))
  let (pathAttachment, noFrame) = linked.wire()
  #expect(noFrame == nil)
  #expect(pathAttachment.path == "/Users/example/report.pdf")
  #expect(pathAttachment.attachmentId == nil && pathAttachment.bytes == "2048")

  let web = AgentWebAttachmentDraft(
    sourcePaneId: "web", package: WebElementPackage(url: "u", title: "t", elements: []),
    images: [
      WebHostAttachment(
        reference: AttachmentRef(
          attachmentId: "00112233-4455-4677-8899-aabbccddeeff", mimeType: "image/png",
          name: "element.png", bytes: "1"), bytes: Data([7]))
    ])
  let (all, frames) = EngineClient.promptAttachments(web: web, drafts: [upload, linked])
  #expect(all.map(\.name) == ["element.png", "shot.png", "report.pdf"])
  #expect(all.first?.kind == .image)
  #expect(frames.count == 2)
}

@Test func filesAreLinkedOnTheEngineMacAndSentAsBytesFromADevice() throws {
  let pdf = try temporaryFile("Report Q3.pdf", contents: Data("%PDF-1.7".utf8))
  let linked = try AgentAttachmentStaging.file(at: pdf, linkHostFiles: true)
  #expect(linked.kind == .file && linked.mimeType == "application/pdf")
  #expect(linked.source == .hostFile(pdf.standardizedFileURL.path))
  #expect(linked.byteCount == 8)

  let sent = try AgentAttachmentStaging.file(at: pdf, linkHostFiles: false)
  #expect(sent.source == .bytes(Data("%PDF-1.7".utf8)))

  let notes = try temporaryFile("notes.md", contents: Data("# Notes".utf8))
  #expect(try AgentAttachmentStaging.file(at: notes, linkHostFiles: true).kind == .text)
  #expect(
    throws: AgentAttachmentRejection.folder(
      name: notes.deletingLastPathComponent().lastPathComponent)
  ) {
    try AgentAttachmentStaging.file(at: notes.deletingLastPathComponent(), linkHostFiles: true)
  }
}

@Test func imageFilesAreAlwaysSentAsPreparedBytesWithAThumbnail() throws {
  let url = try temporaryFile(
    "diagram.png", contents: try picture(width: 400, height: 300, as: .png))
  let image = try AgentAttachmentStaging.file(at: url, linkHostFiles: true)
  #expect(image.kind == .image && image.mimeType == "image/png" && image.name == "diagram.png")
  guard case .bytes(let bytes) = image.source else {
    Issue.record("an image is sent as bytes")
    return
  }
  #expect(bytes == (try Data(contentsOf: url)), "an inline-ready PNG is sent unchanged")
  let thumbnail = try #require(image.thumbnail)
  #expect(thumbnail.count <= ProtocolLimits.promptAttachmentThumbnailBytes)
  #expect(try pixelSize(thumbnail) <= AgentAttachmentStaging.thumbnailPixels)
}

@Test func tiffAndOversizedImagesAreReencodedForTheAgent() throws {
  let tiff = try picture(width: 300, height: 200, as: .tiff)
  let converted = try #require(AgentAttachmentStaging.image(tiff, type: .tiff, name: nil))
  #expect(["image/png", "image/jpeg"].contains(converted.mimeType))
  #expect(converted.name.hasPrefix("Pasted image."))
  #expect(converted.name.hasSuffix(converted.mimeType == "image/png" ? ".png" : ".jpeg"))

  let large = try picture(width: 3_000, height: 2_000, as: .png, noise: true)
  let prepared = try #require(AgentAttachmentStaging.image(large, type: .png, name: "big.png"))
  guard case .bytes(let bytes) = prepared.source else {
    Issue.record("an image is sent as bytes")
    return
  }
  #expect(try pixelSize(bytes) <= AgentAttachmentStaging.maximumImagePixels)
  #expect(bytes.count <= AgentAttachmentStaging.maximumImageBytes)
  #expect(prepared.name.hasPrefix("big."))
  #expect(AgentAttachmentStaging.image(Data("not an image".utf8), type: .png, name: nil) == nil)
}

@Test func pasteAttachesFilesAndImagesAndLeavesTextToTheTextView() throws {
  let png = try picture(width: 40, height: 30, as: .png)
  let screenshot = AgentPasteboardContent(image: .init(data: png, type: .png))
  #expect(screenshot.offersAttachments)
  let drafts = screenshot.drafts(linkHostFiles: true).compactMap { try? $0.get() }
  #expect(drafts.map(\.mimeType) == ["image/png"])

  let copied = try temporaryFile("copied.pdf", contents: Data("%PDF".utf8))
  let finder = AgentPasteboardContent(
    fileURLs: [copied], image: .init(data: png, type: .tiff), hasText: true)
  #expect(finder.offersAttachments, "Finder copies a name and an icon with the file")
  let files = finder.drafts(linkHostFiles: true).compactMap { try? $0.get() }
  #expect(files.map(\.name) == ["copied.pdf"])

  #expect(!AgentPasteboardContent(hasText: true).offersAttachments)
  #expect(
    !AgentPasteboardContent(image: .init(data: png, type: .png), hasText: true).offersAttachments)
  #expect(AgentPasteboardContent(hasText: true).drafts(linkHostFiles: true).isEmpty)
  let broken = AgentPasteboardContent(image: .init(data: Data([1]), type: .png))
  #expect(broken.drafts(linkHostFiles: true).first.map { (try? $0.get()) == nil } == true)
}

@Test func attachmentChipsDescribeKindSizeAndThumbnail() {
  let sent = PromptAttachment(
    kind: .file, name: "report.pdf", mimeType: "application/pdf", bytes: "2048",
    path: "/tmp/report.pdf", thumbnail: Data([1, 2]).base64EncodedString())
  let chip = AgentAttachmentPresentation(attachment: sent, index: 0)
  #expect(chip.systemImage == "doc.richtext")
  #expect(chip.detail.hasPrefix("PDF · "))
  #expect(chip.thumbnail == Data([1, 2]))
  #expect(AgentAttachmentPresentation.systemImage(kind: .image, mimeType: "image/png") == "photo")
  #expect(AgentAttachmentPresentation.detail(mimeType: "image/svg+xml", bytes: 0) == "SVG")
}

#if os(macOS)
  @MainActor @Test func macPasteboardImagesAndFileURLsBecomeAttachments() throws {
    let pasteboard = NSPasteboard(name: NSPasteboard.Name("soyeht-test-\(UUID().uuidString)"))
    defer { pasteboard.releaseGlobally() }

    let tiff = try picture(width: 64, height: 48, as: .tiff)
    pasteboard.clearContents()
    pasteboard.setData(tiff, forType: .tiff)
    let copiedImage = AgentPasteboardContent(pasteboard: pasteboard)
    #expect(copiedImage.image?.type == .tiff && copiedImage.offersAttachments)
    let converted = try #require(try copiedImage.drafts(linkHostFiles: true).first?.get())
    #expect(converted.kind == .image && converted.mimeType != "image/tiff")

    let png = try picture(width: 64, height: 48, as: .png)
    pasteboard.clearContents()
    pasteboard.setData(png, forType: .png)
    pasteboard.setData(tiff, forType: .tiff)
    #expect(AgentPasteboardContent(pasteboard: pasteboard).image?.type == .png)

    let file = try temporaryFile("from finder.txt", contents: Data("text".utf8))
    pasteboard.clearContents()
    pasteboard.writeObjects([file as NSURL])
    pasteboard.setString("from finder.txt", forType: .string)
    let finder = AgentPasteboardContent(pasteboard: pasteboard)
    #expect(finder.fileURLs.map(\.standardizedFileURL) == [file.standardizedFileURL])
    #expect(finder.offersAttachments)
    let linked = try #require(try finder.drafts(linkHostFiles: true).first?.get())
    #expect(linked.source == .hostFile(file.standardizedFileURL.path) && linked.kind == .text)

    pasteboard.clearContents()
    pasteboard.setString("just words", forType: .string)
    #expect(!AgentPasteboardContent(pasteboard: pasteboard).offersAttachments)
  }

  @Test func onlyPlainCommandVIsTheAttachmentPasteKey() {
    #expect(AgentComposerPaste.isPasteKey(characters: "v", modifiers: .command))
    #expect(AgentComposerPaste.isPasteKey(characters: "v", modifiers: [.command, .capsLock]))
    #expect(!AgentComposerPaste.isPasteKey(characters: "v", modifiers: [.command, .shift]))
    #expect(!AgentComposerPaste.isPasteKey(characters: "v", modifiers: [.command, .option]))
    #expect(!AgentComposerPaste.isPasteKey(characters: "c", modifiers: .command))
    #expect(!AgentComposerPaste.isPasteKey(characters: "v", modifiers: []))
  }

  @MainActor @Test func composerPasteAddsChipsAndTakeEmptiesIt() async throws {
    let composer = AgentComposerAttachments(linkHostFiles: true)
    let png = try picture(width: 20, height: 20, as: .png)
    #expect(composer.paste(AgentPasteboardContent(image: .init(data: png, type: .png))))
    #expect(!composer.paste(AgentPasteboardContent(hasText: true)))
    let deadline = Date().addingTimeInterval(10)
    while composer.items.isEmpty && Date() < deadline {
      try await Task.sleep(for: .milliseconds(20))
    }
    #expect(composer.items.count == 1)
    #expect(composer.take().count == 1)
    #expect(composer.items.isEmpty)
  }
#endif

@MainActor @Test func droppedFilesAndImagesAttachButPaneDragsAreLeftAlone() async throws {
  let composer = AgentComposerAttachments(linkHostFiles: true)
  let pane = NSItemProvider()
  pane.registerDataRepresentation(
    forTypeIdentifier: PaneDragTransferPolicy.typeIdentifier, visibility: .ownProcess
  ) { completion in
    completion(Data(), nil)
    return nil
  }
  #expect(!composer.attach(providers: [pane]))
  #expect(!composer.attach(providers: [NSItemProvider(object: "dragged words" as NSString)]))

  let file = try temporaryFile("dropped.pdf", contents: Data("%PDF".utf8))
  let png = try picture(width: 20, height: 20, as: .png)
  let image = NSItemProvider(item: png as NSData, typeIdentifier: UTType.png.identifier)
  let dropped = try #require(NSItemProvider(contentsOf: file))
  #expect(composer.attach(providers: [dropped, image]))
  let deadline = Date().addingTimeInterval(10)
  while composer.items.count < 2 && Date() < deadline {
    try await Task.sleep(for: .milliseconds(20))
  }
  #expect(Set(composer.items.map(\.kind)) == [.file, .image])
  #expect(composer.items.contains { $0.source == .hostFile(file.standardizedFileURL.path) })
}
