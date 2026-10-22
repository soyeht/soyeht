import Foundation
import SoyehtProtocol

public enum MarkdownPreviewError: Error, Equatable {
  case tooManyLines
  case tooManyBytes
}

public struct MarkdownViewportPreview {
  /// The Markdown as blocks: the same parse, and the same drawing, as an agent's reply.
  public let blocks: [MarkdownBlock]
  public let fromLine: UInt32
  public let toLine: UInt32
  public let totalLineCount: UInt32

  public init(lines: [String], fromLine: UInt32, totalLineCount: UInt32) throws {
    guard lines.count <= ProtocolLimits.editorViewportLines else {
      throw MarkdownPreviewError.tooManyLines
    }
    let source = lines.joined(separator: "\n")
    guard source.utf8.count <= ProtocolLimits.jsonBytes else {
      throw MarkdownPreviewError.tooManyBytes
    }
    blocks = MarkdownReply.parse(source)
    self.fromLine = fromLine
    toLine = fromLine.saturatingAdd(UInt32(lines.count))
    self.totalLineCount = totalLineCount
  }

  public var isPartial: Bool {
    fromLine > 0 || toLine < totalLineCount
  }

  /// The lines the preview shows, one-based: `LINES 41–43 OF 100000`.
  public var rangeLabel: String {
    "LINES \(UInt64(fromLine) + 1)–\(toLine) OF \(totalLineCount)"
  }
}

extension UInt32 {
  fileprivate func saturatingAdd(_ value: UInt32) -> UInt32 {
    addingReportingOverflow(value).overflow ? .max : self + value
  }
}
