import Foundation
import SoyehtProtocol
import Testing
import UniformTypeIdentifiers

@testable import SoyehtClient

/// The cases of `contracts/vectors/attachments.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("attachments") }

private let english = Locale(identifier: "en_US")

@Test func sizesAreWrittenAsFinderWritesThem() throws {
  for item in ContractVector.cases(try vector(), "fileSize") {
    let bytes = try #require(item["bytes"].int)
    #expect(
      AgentAttachmentPresentation.fileSize(bytes, locale: english) == item["text"].string,
      "\(bytes)")
  }
}

@Test func theWrittenOutSizeRuleIsTheSystemFormattersInTheCurrentLocale() throws {
  let separator = Locale.current.decimalSeparator ?? "."
  for item in ContractVector.cases(try vector(), "fileSize") {
    let bytes = try #require(item["bytes"].int)
    let system = ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    #expect(AgentAttachmentPresentation.fileSize(bytes) == system, "\(bytes) as \(separator)")
  }
}

@Test func aChipSaysTheFormatAndTheSize() throws {
  for item in ContractVector.cases(try vector(), "detail") {
    let mime = item["mimeType"].string ?? ""
    let bytes = item["bytes"].int ?? 0
    #expect(
      AgentAttachmentPresentation.detail(mimeType: mime, bytes: bytes, locale: english)
        == item["text"].string,
      "\(mime)")
  }
  for item in ContractVector.cases(try vector(), "glyph") {
    let kind = try item["kind"].decoded(PromptAttachmentKind.self)
    let glyph = AgentAttachmentGlyph(kind: kind, mimeType: item["mimeType"].string ?? "")
    #expect(glyph.rawValue == item["glyph"].string, "\(item["mimeType"].string ?? "")")
  }
}

@Test func filesAreSortedByTheirNameTheWayTheDesktopSortsThem() throws {
  for item in ContractVector.cases(try vector(), "fileKinds") {
    let name = item["fileName"].string ?? ""
    let type = UTType(filenameExtension: (name as NSString).pathExtension) ?? .data
    let kind = AgentAttachmentStaging.kind(of: type)
    let expected = try item["kind"].decoded(PromptAttachmentKind.self)
    #expect(kind == expected, "\(name)")
    #expect(
      AgentAttachmentStaging.mimeType(of: type, kind: kind) == item["mimeType"].string, "\(name)")
  }
}

@Test func aPasteAttachesFilesAndPicturesButLeavesTextToTheField() throws {
  for item in ContractVector.cases(try vector(), "pasteOffers") {
    let files = (0..<(item["files"].int ?? 0)).map { URL(fileURLWithPath: "/tmp/f\($0).txt") }
    let image =
      item["image"].bool == true ? AgentPasteboardContent.Image(data: Data([1]), type: .png) : nil
    let content = AgentPasteboardContent(
      fileURLs: files, image: image, hasText: item["text"].bool == true)
    #expect(content.offersAttachments == (item["offers"].bool == true), "\(item)")
  }
}

@Test func theImagePolicyIsTheSharedOne() throws {
  let policy = try vector()["imagePolicy"]
  #expect(
    AgentAttachmentStaging.inlineImageTypes.compactMap(\.preferredMIMEType)
      == (policy["inlineTypes"].array ?? []).compactMap(\.string))
  #expect(AgentAttachmentStaging.maximumImagePixels == policy["maximumPixels"].int)
  #expect(AgentAttachmentStaging.maximumImageBytes == policy["maximumBytes"].int)
  #expect(AgentAttachmentStaging.thumbnailPixels == policy["thumbnailPixels"].int)
  #expect(AgentAttachmentStaging.fallbackImageName == policy["fallbackName"].string)
  #expect(
    AgentAttachmentStaging.scalePixels == (policy["scalePixels"].array ?? []).compactMap(\.int))
  #expect(
    AgentAttachmentStaging.jpegQualities
      == (policy["qualities"].array ?? []).compactMap(\.double))
  let attempts = policy["thumbnailAttempts"].array ?? []
  #expect(AgentAttachmentStaging.thumbnailAttempts.count == attempts.count)
  for (attempt, expected) in zip(AgentAttachmentStaging.thumbnailAttempts, attempts) {
    #expect(attempt.0 == expected["pixels"].int)
    #expect(attempt.1 == expected["quality"].double)
  }
  for item in ContractVector.cases(try vector(), "keepsImage") {
    let type = try #require(UTType(mimeType: item["mimeType"].string ?? ""))
    #expect(
      AgentAttachmentStaging.keepsOriginal(
        type: type, byteCount: item["bytes"].int ?? 0, width: item["width"].int ?? 0,
        height: item["height"].int ?? 0) == (item["keeps"].bool == true),
      "\(item)")
  }
  for item in ContractVector.cases(try vector(), "imageName") {
    let type = try #require(UTType(mimeType: item["sentAs"].string ?? ""))
    #expect(
      AgentAttachmentStaging.imageFileName(item["name"].string, type: type)
        == item["fileName"].string,
      "\(item)")
  }
}

private func outcomeName(of rejection: AgentAttachmentRejection) -> String {
  switch rejection {
  case .tooMany: "tooMany"
  case .tooLarge: "tooLarge"
  case .totalTooLarge: "totalTooLarge"
  case .empty: "empty"
  case .unreadable: "unreadable"
  case .folder: "folder"
  }
}

private func rejection(_ item: JsonValue) throws -> AgentAttachmentRejection {
  let name = item["name"].string ?? ""
  return switch item["rejection"].string {
  case "tooMany": .tooMany
  case "tooLarge": .tooLarge(name: name)
  case "totalTooLarge": .totalTooLarge
  case "empty": .empty(name: name)
  case "unreadable": .unreadable(name: name)
  case "folder": .folder(name: name)
  default: throw ClientFailure.invalidFrame
  }
}

@Test func aMessageTakesAtMostTheEnginesLimitsAndSaysWhyNot() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "limits") {
    var list = AgentAttachmentList()
    var outcomes: [String] = []
    for add in item["adds"].array ?? [] {
      let size = add["bytes"].int ?? 0
      let name = add["name"].string ?? ""
      let draft = AgentAttachmentDraft(
        kind: .file, name: name, mimeType: "application/octet-stream", byteCount: size,
        source: add["linked"].bool == true ? .hostFile("/tmp/\(name)") : .bytes(Data(count: size)))
      do {
        try list.add(draft)
        outcomes.append("ok")
      } catch {
        outcomes.append(outcomeName(of: error))
      }
    }
    #expect(
      outcomes == (item["outcomes"].array ?? []).compactMap(\.string),
      "\(item["name"].string ?? "")")
  }
  for item in ContractVector.cases(all, "rejections") {
    #expect(try rejection(item).message == item["message"].string)
  }
}

@Test func aDraftDescribesItselfToTheEngine() throws {
  for item in ContractVector.cases(try vector(), "wire") {
    let name = item["name"].string ?? ""
    let wanted = item["draft"]
    let source: AgentAttachmentDraft.Source =
      if let path = wanted["source"]["path"].string {
        .hostFile(path)
      } else {
        .bytes(try #require(Data(base64Encoded: wanted["source"]["bytes"].string ?? "")))
      }
    let draft = AgentAttachmentDraft(
      id: try #require(UUID(uuidString: wanted["id"].string ?? "")),
      kind: try wanted["kind"].decoded(PromptAttachmentKind.self),
      name: wanted["name"].string ?? "",
      mimeType: wanted["mimeType"].string ?? "", byteCount: wanted["byteCount"].int ?? 0,
      source: source, thumbnail: wanted["thumbnail"].string.flatMap { Data(base64Encoded: $0) })
    let (attachment, frame) = draft.wire()
    #expect(try JsonValue.encoded(attachment) == item["prompt"], "\(name)")
    if case .object = item["frame"] {
      let reference = try #require(frame?.reference)
      #expect(reference.attachmentId == item["frame"]["attachmentId"].string, "\(name)")
      #expect(reference.mimeType == item["frame"]["mimeType"].string, "\(name)")
      #expect(reference.name == item["frame"]["name"].string, "\(name)")
      #expect(reference.bytes == item["frame"]["bytes"].string, "\(name)")
      #expect(frame?.bytes.base64EncodedString() == item["frame"]["data"].string, "\(name)")
    } else {
      #expect(frame == nil, "\(name)")
    }
  }
}
