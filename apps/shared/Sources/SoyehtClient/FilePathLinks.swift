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
  private let base: AttributedString
  private let workspaceId: String
  @Environment(\.filePathOpenAction) private var openFile
  @Environment(\.webLinkOpenAction) private var openWeb

  public init(_ text: String, workspaceId: String) {
    self.init(AttributedString(text), workspaceId: workspaceId)
  }

  /// Text that already carries its own styling (a Markdown reply): file references and web
  /// addresses are linked in the stretches that have no link yet.
  public init(_ text: AttributedString, workspaceId: String) {
    base = text
    self.workspaceId = workspaceId
  }

  public var body: some View {
    Text(attributedText)
      .environment(
        \.openURL,
        OpenURLAction { url in
          if let reference = Self.reference(from: url) {
            openFile(workspaceId: workspaceId, reference: reference)
            return .handled
          }
          guard ["http", "https"].contains(url.scheme?.lowercased()) else { return .discarded }
          openWeb(workspaceId: workspaceId, url: url)
          return .handled
        })
  }

  private var attributedText: AttributedString {
    guard openFile.enabled || openWeb.enabled else { return base }
    var result = base
    var offset = 0
    for segment in FilePathLinks.segments(in: String(base.characters)) {
      switch segment {
      case .plain(let value):
        if openWeb.enabled { linkWebAddresses(in: value, offset: offset, of: &result) }
        offset += value.count
      case .reference(let label, let reference):
        link(offset..<(offset + label.count), to: Self.url(for: reference), in: &result)
        offset += label.count
      }
    }
    return result
  }

  private func linkWebAddresses(in value: String, offset: Int, of result: inout AttributedString) {
    for link in WebLinkRouting.links(in: value) {
      let start = offset + value.distance(from: value.startIndex, to: link.range.lowerBound)
      self.link(start..<(start + link.label.count), to: link.url, in: &result)
    }
  }

  /// Links the characters in `span` unless the author already linked them.
  private func link(_ span: Range<Int>, to url: URL?, in text: inout AttributedString) {
    let start = text.index(text.startIndex, offsetByCharacters: span.lowerBound)
    let end = text.index(start, offsetByCharacters: span.count)
    for run in text[start..<end].runs where run.link == nil {
      text[run.range].link = url
    }
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
