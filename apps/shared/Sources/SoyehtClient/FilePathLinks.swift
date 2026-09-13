import Foundation
import SwiftUI

public struct FilePathReference: Equatable, Sendable {
  public let path: String
  public let line: UInt32
  public let column: UInt32?

  public init(path: String, line: UInt32, column: UInt32? = nil) {
    self.path = path
    self.line = line
    self.column = column
  }

  public static func parse(_ value: String) -> FilePathReference? {
    let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
    let range = NSRange(text.startIndex..<text.endIndex, in: text)
    guard let match = FilePathLinks.matcher?.firstMatch(in: text, range: range),
      match.range == range,
      let pathRange = Range(match.range(at: 1), in: text),
      let lineRange = Range(match.range(at: 2), in: text),
      let line = UInt32(text[lineRange]), line > 0
    else { return nil }
    let rawPath = String(text[pathRange])
    let path = rawPath.hasPrefix("./") ? String(rawPath.dropFirst(2)) : rawPath
    guard FilePathLinks.valid(path) else { return nil }
    let column: UInt32?
    if match.range(at: 3).location == NSNotFound {
      column = nil
    } else {
      guard let columnRange = Range(match.range(at: 3), in: text),
        let value = UInt32(text[columnRange]), value > 0
      else { return nil }
      column = value - 1
    }
    return FilePathReference(path: path, line: line - 1, column: column)
  }
}

public enum FilePathTextSegment: Equatable, Sendable {
  case plain(String)
  case reference(String, FilePathReference)
}

public enum FilePathLinks {
  static let matcher = try? NSRegularExpression(
    pattern:
      #"(?<![A-Za-z0-9_./-])((?:\./)?(?:(?:[A-Za-z0-9_@+.-]+/)+[A-Za-z0-9_@+.-]+|[A-Za-z0-9_@+.-]+\.[A-Za-z0-9_@+.-]+)):([1-9][0-9]*)(?::([1-9][0-9]*))?(?![A-Za-z0-9_:])"#
  )
  private static let maximumLinks = 128
  private static let maximumPathBytes = 4_096

  public static func segments(in text: String) -> [FilePathTextSegment] {
    guard let matcher else { return [.plain(text)] }
    let range = NSRange(text.startIndex..<text.endIndex, in: text)
    let matches = matcher.matches(in: text, range: range).prefix(maximumLinks)
    guard !matches.isEmpty else { return [.plain(text)] }
    var result: [FilePathTextSegment] = []
    var cursor = text.startIndex
    for match in matches {
      guard let matchedRange = Range(match.range, in: text) else { continue }
      if cursor < matchedRange.lowerBound {
        result.append(.plain(String(text[cursor..<matchedRange.lowerBound])))
      }
      let label = String(text[matchedRange])
      if let reference = FilePathReference.parse(label) {
        result.append(.reference(label, reference))
      } else {
        result.append(.plain(label))
      }
      cursor = matchedRange.upperBound
    }
    if cursor < text.endIndex { result.append(.plain(String(text[cursor...]))) }
    return result
  }

  static func valid(_ path: String) -> Bool {
    guard !path.isEmpty, path.utf8.count <= maximumPathBytes, !path.hasPrefix("/") else {
      return false
    }
    return path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy {
      !$0.isEmpty && $0 != "." && $0 != ".."
    }
  }
}

public struct FilePathOpenAction: Sendable {
  private let handler: (@MainActor @Sendable (String, FilePathReference) -> Void)?

  public init(_ handler: @escaping @MainActor @Sendable (String, FilePathReference) -> Void) {
    self.handler = handler
  }

  fileprivate init() { handler = nil }

  fileprivate var enabled: Bool { handler != nil }

  @MainActor public func callAsFunction(workspaceId: String, reference: FilePathReference) {
    handler?(workspaceId, reference)
  }
}

private struct FilePathOpenActionKey: EnvironmentKey {
  static let defaultValue = FilePathOpenAction()
}

extension EnvironmentValues {
  public var filePathOpenAction: FilePathOpenAction {
    get { self[FilePathOpenActionKey.self] }
    set { self[FilePathOpenActionKey.self] = newValue }
  }
}

public struct FilePathText: View {
  private let text: String
  private let workspaceId: String
  @Environment(\.filePathOpenAction) private var openFile

  public init(_ text: String, workspaceId: String) {
    self.text = text
    self.workspaceId = workspaceId
  }

  public var body: some View {
    Text(attributedText)
      .environment(
        \.openURL,
        OpenURLAction { url in
          guard let reference = Self.reference(from: url) else { return .discarded }
          openFile(workspaceId: workspaceId, reference: reference)
          return .handled
        })
  }

  private var attributedText: AttributedString {
    guard openFile.enabled else { return AttributedString(text) }
    var result = AttributedString()
    for segment in FilePathLinks.segments(in: text) {
      switch segment {
      case .plain(let value): result += AttributedString(value)
      case .reference(let label, let reference):
        var value = AttributedString(label)
        value.link = Self.url(for: reference)
        result += value
      }
    }
    return result
  }

  private static func url(for reference: FilePathReference) -> URL? {
    var components = URLComponents()
    components.scheme = "soyeht-file"
    components.host = "open"
    components.queryItems = [
      URLQueryItem(name: "path", value: reference.path),
      URLQueryItem(name: "line", value: String(reference.line)),
      URLQueryItem(name: "column", value: reference.column.map(String.init)),
    ]
    return components.url
  }

  private static func reference(from url: URL) -> FilePathReference? {
    guard url.scheme == "soyeht-file", url.host == "open",
      let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
    else { return nil }
    let values = Dictionary(
      items.compactMap { item in item.value.map { (item.name, $0) } },
      uniquingKeysWith: { first, _ in first })
    guard let path = values["path"], FilePathLinks.valid(path),
      let lineText = values["line"], let line = UInt32(lineText)
    else { return nil }
    let column = values["column"].flatMap(UInt32.init)
    return FilePathReference(path: path, line: line, column: column)
  }
}
