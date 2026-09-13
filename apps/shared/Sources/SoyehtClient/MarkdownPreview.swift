import Foundation
import SoyehtProtocol

public enum MarkdownPreviewError: Error, Equatable {
  case tooManyLines
  case tooManyBytes
  case invalidMarkup
}

public struct MarkdownViewportPreview {
  public let content: AttributedString
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
    do {
      var rendered = try AttributedString(
        markdown: source,
        options: .init(interpretedSyntax: .full, failurePolicy: .returnPartiallyParsedIfPossible))
      for run in rendered.runs where run.link != nil {
        rendered[run.range].link = nil
      }
      content = rendered
    } catch {
      throw MarkdownPreviewError.invalidMarkup
    }
    self.fromLine = fromLine
    toLine = fromLine.saturatingAdd(UInt32(lines.count))
    self.totalLineCount = totalLineCount
  }

  public var isPartial: Bool {
    fromLine > 0 || toLine < totalLineCount
  }
}

extension UInt32 {
  fileprivate func saturatingAdd(_ value: UInt32) -> UInt32 {
    addingReportingOverflow(value).overflow ? .max : self + value
  }
}
