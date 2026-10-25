import Foundation
import SoyehtProtocol
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

  /// The reference a label stands for, read the way the workspace's system writes paths.
  public static func parse(_ value: String, dialect: FilePathDialect = .posix) -> FilePathReference?
  {
    let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
    let range = NSRange(text.startIndex..<text.endIndex, in: text)
    guard let match = FilePathLinks.matcher(for: dialect)?.firstMatch(in: text, range: range),
      match.range == range,
      let parts = FilePathLinks.parts(of: match, in: text, dialect: dialect),
      let line = UInt32(parts.line), line > 0,
      let path = FilePathLinks.path(parts.path, dialect: dialect)
    else { return nil }
    let column: UInt32?
    if let columnText = parts.column {
      guard let value = UInt32(columnText), value > 0 else { return nil }
      column = value - 1
    } else {
      column = nil
    }
    return FilePathReference(path: path, line: line - 1, column: column)
  }
}

/// How the paths of a workspace are written, which is the system the workspace is on. A Mac,
/// iPhone or iPad workspace is `.posix`. `.windows` is what a client connected to a Windows
/// engine reads (the desktop client reads it on Windows): `\` separates like `/`, a drive or a
/// UNC share makes a path absolute, a path may be quoted to hold example, and the MSVC
/// `file(line,column)` is a location. Both are held together by `contracts/vectors/file-paths.json`.
public enum FilePathDialect: Sendable, CaseIterable {
  case posix
  case windows
}

public enum FilePathTextSegment: Equatable, Sendable {
  case plain(String)
  case reference(String, FilePathReference)
}

public enum FilePathLinks {
  static let posixMatcher = try? NSRegularExpression(
    pattern:
      #"(?<![A-Za-z0-9_./-])((?:\./)?(?:(?:[A-Za-z0-9_@+.-]+/)+[A-Za-z0-9_@+.-]+|[A-Za-z0-9_@+.-]+\.[A-Za-z0-9_@+.-]+)):([1-9][0-9]*)(?::([1-9][0-9]*))?(?![A-Za-z0-9_:])"#
  )

  /// The windows dialect: a quoted path (groups 1 to 4: quote, path, line, column) or an
  /// unquoted one (groups 5 to 9: path, line, column, and the MSVC line and column).
  static let windowsMatcher: NSRegularExpression? = {
    let name = "[A-Za-z0-9_@+.-]+"
    let separator = #"[\\/]"#
    let root =
      #"(?:[A-Za-z]:\#(separator)|\\\\\#(name)\#(separator)\#(name)\#(separator)|\.\#(separator))"#
    let unquoted =
      #"(?<![A-Za-z0-9_./\\-])(\#(root)?(?:(?:\#(name)\#(separator))+\#(name)|\#(name)\.\#(name)))"#
      + #"(?::([1-9][0-9]*)(?::([1-9][0-9]*))?(?![A-Za-z0-9_:])|\(([1-9][0-9]*)(?:,([1-9][0-9]*))?\))"#
    let part = #"[^"'<>|*?:\\/\p{Cc}]+"#
    let quotedRoot =
      #"(?:[A-Za-z]:\#(separator)|\\\\\#(part)\#(separator)\#(part)\#(separator)|\.\#(separator))"#
    let quoted =
      #"(?<![A-Za-z0-9_./\\-])(["'])(?=[^"'\p{Cc}]*?[^A-Za-z0-9_@+.\\/:"'\p{Cc}-])"#
      + #"(\#(quotedRoot)?(?:(?:\#(part)\#(separator))+\#(part)|\#(part)\.\#(part))):([1-9][0-9]*)(?::([1-9][0-9]*))?\1"#
    do {
      return try NSRegularExpression(pattern: "\(quoted)|\(unquoted)")
    } catch {
      assertionFailure("The windows file pattern does not compile: \(error)")
      return nil
    }
  }()

  static func matcher(for dialect: FilePathDialect) -> NSRegularExpression? {
    dialect == .windows ? windowsMatcher : posixMatcher
  }

  /// The path, line and column texts of a match.
  static func parts(of match: NSTextCheckingResult, in text: String, dialect: FilePathDialect)
    -> (path: String, line: String, column: String?)?
  {
    func group(_ index: Int) -> String? {
      Range(match.range(at: index), in: text).map { String(text[$0]) }
    }
    switch dialect {
    case .posix:
      guard let path = group(1), let line = group(2) else { return nil }
      return (path, line, group(3))
    case .windows:
      if let path = group(2), let line = group(3) { return (path, line, group(4)) }
      guard let path = group(5), let line = group(6) ?? group(8) else { return nil }
      return (path, line, group(6) != nil ? group(7) : group(9))
    }
  }

  public static func segments(in text: String, dialect: FilePathDialect = .posix)
    -> [FilePathTextSegment]
  {
    guard let matcher = matcher(for: dialect) else { return [.plain(text)] }
    let range = NSRange(text.startIndex..<text.endIndex, in: text)
    let matches = matcher.matches(in: text, range: range).prefix(ClientPolicy.capsFileLinks)
    guard !matches.isEmpty else { return [.plain(text)] }
    var result: [FilePathTextSegment] = []
    var cursor = text.startIndex
    for match in matches {
      guard let matchedRange = Range(match.range, in: text) else { continue }
      if cursor < matchedRange.lowerBound {
        result.append(.plain(String(text[cursor..<matchedRange.lowerBound])))
      }
      let label = String(text[matchedRange])
      if let reference = FilePathReference.parse(label, dialect: dialect) {
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
    guard !path.isEmpty, path.utf8.count <= ProtocolLimits.editorPathBytes, !path.hasPrefix("/")
    else {
      return false
    }
    return path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy {
      !$0.isEmpty && $0 != "." && $0 != ".."
    }
  }

  /// The path a label names, with `/` between components and no leading `./`, or nil when it only
  /// looks like a reference. In the windows dialect a drive (`C:/`) or a share (`//server/share/`)
  /// is kept as written as the root of an absolute path.
  static func path(_ raw: String, dialect: FilePathDialect) -> String? {
    guard dialect == .windows else {
      let path = raw.hasPrefix("./") ? String(raw.dropFirst(2)) : raw
      return valid(path) ? path : nil
    }
    let normal = raw.replacingOccurrences(of: "\\", with: "/")
    var root = ""
    var rest = Substring(normal)
    let letters = Array(raw)
    if letters.count >= 3, letters[0].isASCII, letters[0].isLetter, letters[1] == ":",
      letters[2] == "\\" || letters[2] == "/"
    {
      root = String(normal.prefix(3))
      rest = normal.dropFirst(3)
    } else if raw.hasPrefix("\\\\") {
      let parts = normal.dropFirst(2).split(
        separator: "/", maxSplits: 2, omittingEmptySubsequences: false)
      if parts.count == 3, ![".", ".."].contains(parts[0]), ![".", ".."].contains(parts[1]),
        !parts[0].isEmpty, !parts[1].isEmpty
      {
        root = "//\(parts[0])/\(parts[1])/"
        rest = parts[2]
      }
    }
    var path = String(rest)
    if root.isEmpty, path.hasPrefix("./") { path.removeFirst(2) }
    guard root.utf8.count + path.utf8.count <= ProtocolLimits.editorPathBytes,
      valid(path)
    else { return nil }
    return root + path
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
