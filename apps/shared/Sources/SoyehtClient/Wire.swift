import Foundation
import Network
import SoyehtProtocol

public enum ClientFailure: Error, LocalizedError, Sendable {
  case disconnected, invalidFrame, invalidSnapshot, invalidStream, timeout
  case editorCommand(String)
  case webHost(ErrorKind, String)
  case remote(String, String)
  var canRetry: Bool {
    switch self {
    case .disconnected, .timeout: true
    default: false
    }
  }
  public var errorDescription: String? {
    switch self {
    case .disconnected: "Connection interrupted. Reconnecting…"
    case .invalidFrame: "The engine sent an invalid message."
    case .invalidSnapshot: "Received state is incomplete."
    case .invalidStream: "The screen needs to synchronize again."
    case .timeout: "The engine did not respond in time."
    case .editorCommand(let message): message
    case .webHost(_, let message): message
    case .remote(_, let message): message
    }
  }
}

extension JsonValue {
  subscript(_ key: String) -> JsonValue {
    guard case .object(let value) = self else { return .null }
    return value[key] ?? .null
  }
  var string: String? { if case .string(let value) = self { value } else { nil } }
  var array: [JsonValue]? { if case .array(let value) = self { value } else { nil } }
  var int: Int? { if case .integer(let value) = self { Int(exactly: value) } else { nil } }
  func decoded<T: Decodable>(_ type: T.Type) throws -> T {
    try JSONDecoder().decode(type, from: JSONEncoder().encode(self))
  }
  static func encoded<T: Encodable>(_ value: T) throws -> JsonValue {
    try JSONDecoder().decode(JsonValue.self, from: JSONEncoder().encode(value))
  }
}

/// Owns one connection. All reads and writes are asynchronous; no UI thread waits on a descriptor.
@MainActor
final class Wire {
  private struct ReadWaiter {
    let id: UInt64
    let continuation: CheckedContinuation<Data, Error>
  }

  private let connection: NWConnection?
  private var webSocket: URLSessionWebSocketTask?
  private var session: URLSession?
  private var greetingDeadline: Task<Void, Never>?
  private var openWaiter: CheckedContinuation<Void, Error>?
  private var readWaiter: ReadWaiter?
  private var nextReadId: UInt64 = 0
  private var terminalFailure: Error?
  private var opened = false
  private var closed = false

  init(socket: String) {
    connection = NWConnection(to: .unix(path: socket), using: .tcp)
  }

  init(url: URL) {
    connection = nil
    let session = URLSession(configuration: .ephemeral)
    self.session = session
    let task = session.webSocketTask(with: url)
    task.maximumMessageSize = 1_048_576
    webSocket = task
  }

  func open() async throws {
    greetingDeadline = Task { [weak self] in
      do { try await Task.sleep(for: .seconds(5)) } catch { return }
      self?.close()
    }
    if let webSocket {
      webSocket.resume()
      return
    }
    guard let connection else { throw ClientFailure.disconnected }
    try await withCheckedThrowingContinuation { continuation in
      openWaiter = continuation
      connection.stateUpdateHandler = { [weak self] state in
        Task { @MainActor in
          guard let self else { return }
          switch state {
          case .ready:
            self.opened = true
            self.openWaiter?.resume()
            self.openWaiter = nil
          case .failed(let error), .waiting(let error):
            self.fail(self.opened ? ClientFailure.disconnected : error)
          case .cancelled:
            self.fail(ClientFailure.disconnected)
          default: break
          }
        }
      }
      connection.start(queue: DispatchQueue(label: "com.soyeht.next.dev.client.socket"))
    }
  }

  func close() {
    closed = true
    connection?.cancel()
    webSocket?.cancel(with: .goingAway, reason: nil)
    session?.invalidateAndCancel()
    greetingDeadline?.cancel()
    greetingDeadline = nil
    fail(ClientFailure.disconnected)
  }

  func send(_ value: JsonValue, attachments: [WebHostAttachment] = []) async throws {
    guard !closed, terminalFailure == nil else { throw ClientFailure.disconnected }
    let frames = try WirePayload(message: value, attachments: attachments).frames()
    if let webSocket {
      for frame in frames {
        if frame.kind == 1 {
          try await webSocket.send(.string(String(decoding: frame.payload, as: UTF8.self)))
        } else {
          try await webSocket.send(.data(try frame.encode()))
        }
      }
      return
    }
    guard let connection else { throw ClientFailure.disconnected }
    var data = Data()
    for frame in frames { data.append(try frame.encode()) }
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
      connection.send(
        content: data,
        completion: .contentProcessed { error in
          if error != nil {
            continuation.resume(throwing: ClientFailure.disconnected)
          } else {
            continuation.resume()
          }
        })
    }
  }

  func send(_ response: WebHostResponse) async throws {
    guard !closed, terminalFailure == nil else { throw ClientFailure.disconnected }
    let frames = try response.frames()
    if let webSocket {
      for frame in frames {
        if frame.kind == 1 {
          try await webSocket.send(.string(String(decoding: frame.payload, as: UTF8.self)))
        } else {
          try await webSocket.send(.data(try frame.encode()))
        }
      }
      return
    }
    guard let connection else { throw ClientFailure.disconnected }
    var data = Data()
    for frame in frames { data.append(try frame.encode()) }
    try await withCheckedThrowingContinuation {
      (continuation: CheckedContinuation<Void, Error>) in
      connection.send(
        content: data,
        completion: .contentProcessed { error in
          if error != nil {
            continuation.resume(throwing: ClientFailure.disconnected)
          } else {
            continuation.resume()
          }
        })
    }
  }

  private func exact(_ count: Int) async throws -> Data {
    guard let connection else { throw ClientFailure.disconnected }
    var result = Data()
    while result.count < count {
      guard !closed, terminalFailure == nil else { throw ClientFailure.disconnected }
      let remaining = count - result.count
      nextReadId &+= 1
      let readId = nextReadId
      let chunk: Data = try await withCheckedThrowingContinuation { continuation in
        readWaiter = ReadWaiter(id: readId, continuation: continuation)
        connection.receive(minimumIncompleteLength: 1, maximumLength: remaining) {
          data, _, _, error in
          Task { @MainActor [weak self] in
            self?.finishRead(readId, data: data, error: error)
          }
        }
      }
      result.append(chunk)
    }
    return result
  }

  private func finishRead(_ id: UInt64, data: Data?, error: Error?) {
    guard readWaiter?.id == id, let waiter = readWaiter else { return }
    readWaiter = nil
    guard error == nil, let data, !data.isEmpty else {
      waiter.continuation.resume(throwing: ClientFailure.disconnected)
      return
    }
    waiter.continuation.resume(returning: data)
  }

  private func fail(_ error: Error) {
    if terminalFailure == nil { terminalFailure = error }
    if let waiter = openWaiter {
      openWaiter = nil
      waiter.resume(throwing: error)
    }
    if let waiter = readWaiter {
      readWaiter = nil
      waiter.continuation.resume(throwing: ClientFailure.disconnected)
    }
  }

  func next() async throws -> Frame {
    let frame = try await readFrame()
    greetingDeadline?.cancel()
    greetingDeadline = nil
    return frame
  }

  private func readFrame() async throws -> Frame {
    if let webSocket {
      switch try await webSocket.receive() {
      case .string(let text):
        guard !text.isEmpty, text.utf8.count <= 1_048_576 else { throw ClientFailure.invalidFrame }
        return Frame(kind: 1, payload: Data(text.utf8))
      case .data(let bytes):
        let frame = try Frame.decode(bytes)
        guard frame.kind != 1 else { throw ClientFailure.invalidFrame }
        return frame
      @unknown default: throw ClientFailure.invalidFrame
      }
    }
    let prefix = try await exact(5)
    let size = prefix.dropFirst().reduce(0) { ($0 << 8) | Int($1) }
    // Validate before allocating or reading the advertised body.
    let range: ClosedRange<Int>
    switch prefix.first {
    case 1: range = 1...1_048_576
    case 2: range = 16...(16 + 65_536)
    case 3: range = 16...(16 + 262_144)
    default: throw ClientFailure.invalidFrame
    }
    guard range.contains(size) else { throw ClientFailure.invalidFrame }
    return try Frame.decode(prefix + (try await exact(size)))
  }
}

struct WirePayload {
  let message: JsonValue
  let attachments: [WebHostAttachment]

  func frames(maxAttachments: Int = 20) throws -> [Frame] {
    guard attachments.count <= maxAttachments else { throw ClientFailure.invalidFrame }
    var frames: [Frame] = []
    for attachment in attachments {
      guard !attachment.bytes.isEmpty,
        attachment.reference.bytes == String(attachment.bytes.count),
        attachment.bytes.count <= ProtocolLimits.webScreenshotBytes,
        let uuid = UUID(uuidString: attachment.reference.attachmentId),
        uuid.uuidString.lowercased() == attachment.reference.attachmentId
      else { throw ClientFailure.invalidFrame }
      let header = withUnsafeBytes(of: uuid.uuid) { Data($0) }
      var offset = 0
      repeat {
        let end = min(offset + ProtocolLimits.attachmentPayloadBytes, attachment.bytes.count)
        frames.append(
          Frame(kind: 3, header: header, payload: attachment.bytes.subdata(in: offset..<end)))
        offset = end
      } while offset < attachment.bytes.count
    }
    let json = try JSONEncoder().encode(message)
    guard json.count <= ProtocolLimits.jsonBytes else { throw ClientFailure.invalidFrame }
    frames.append(Frame(kind: 1, payload: json))
    return frames
  }
}

struct WebHostResponse: Sendable {
  let message: JsonValue
  let attachments: [WebHostAttachment]

  init(message: JsonValue, attachments: [WebHostAttachment] = []) {
    self.message = message
    self.attachments = attachments
  }

  func frames() throws -> [Frame] {
    try WirePayload(message: message, attachments: attachments).frames(maxAttachments: 1)
  }
}
