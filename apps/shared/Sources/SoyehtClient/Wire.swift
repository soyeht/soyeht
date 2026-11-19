import Foundation
import Network
import SoyehtProtocol

public enum ClientFailure: Error, LocalizedError, Sendable {
  case disconnected, invalidFrame, invalidSnapshot, invalidStream, timeout
  /// The engine welcomed this app at a protocol version it does not speak (ADR 0161).
  case unsupportedProtocolVersion
  case requestTimeout(String)
  case editorCommand(String)
  case webHost(ErrorKind, String)
  case remote(String, String, retryAfterSec: UInt32? = nil)
  var canRetry: Bool {
    switch self {
    case .disconnected, .timeout, .requestTimeout: true
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
    case .unsupportedProtocolVersion: ClientPolicy.updateSoyeht
    case .requestTimeout(let method): "The engine did not respond to \(method) in time."
    case .editorCommand(let message): message
    case .webHost(_, let message): message
    case .remote(_, let message, _): message
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
  /// The notification a `method` field names, when it is one of the contract's events.
  var eventName: EventName? { string.flatMap(EventName.init(rawValue:)) }
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
  /// A unit that fails on the transport leaves the engine with frames it never saw committed, so
  /// the connection ends with it.
  private lazy var sendQueue = SendQueue<[Outgoing]>(
    transmit: { [weak self] unit in
      guard let self else { throw ClientFailure.disconnected }
      try await self.transmit(unit)
    },
    onFailure: { [weak self] _ in self?.close() })

  init(socket: String) {
    connection = NWConnection(to: .unix(path: socket), using: .tcp)
  }

  init(url: URL) {
    connection = nil
    let session = URLSession(configuration: .ephemeral)
    self.session = session
    let task = session.webSocketTask(with: url)
    task.maximumMessageSize = ProtocolLimits.jsonBytes
    webSocket = task
  }

  func open(greetingTimeout: Duration = greetingTimeout) async throws {
    greetingDeadline = Task { [weak self] in
      do { try await Task.sleep(for: greetingTimeout) } catch { return }
      self?.close(ClientFailure.timeout)
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

  /// Ends the connection. `reason` is what a pending or later read reports, so a deadline says
  /// "no answer in time" instead of the transport's own "cancelled".
  func close(_ reason: ClientFailure = .disconnected) {
    closed = true
    fail(reason)
    sendQueue.fail(reason)
    connection?.cancel()
    webSocket?.cancel(with: .goingAway, reason: nil)
    session?.invalidateAndCancel()
    greetingDeadline?.cancel()
    greetingDeadline = nil
  }

  func send(_ value: JsonValue, attachments: [WebHostAttachment] = []) async throws {
    try await write(WirePayload(message: value, attachments: attachments).frames())
  }

  func send(_ response: WebHostResponse) async throws {
    try await write(response.frames())
  }

  /// Queues the frames of one message as a unit and returns when the unit has left: another
  /// message never lands between them (`SendQueue`).
  private func write(_ frames: [Frame]) async throws {
    guard !closed, terminalFailure == nil else { throw ClientFailure.disconnected }
    try await sendQueue.send(try Outgoing.unit(frames, singleWrite: webSocket == nil))
  }

  private func transmit(_ unit: [Outgoing]) async throws {
    guard !closed, terminalFailure == nil else { throw ClientFailure.disconnected }
    if let webSocket {
      do {
        for message in unit {
          switch message {
          case .text(let text): try await webSocket.send(.string(text))
          case .binary(let data): try await webSocket.send(.data(data))
          }
        }
      } catch { throw transportFailure(error) }
      return
    }
    guard let connection else { throw ClientFailure.disconnected }
    var data = Data()
    for case .binary(let bytes) in unit { data.append(bytes) }
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

  /// A socket this wire closed reports `URLError.cancelled`, which reads "cancelled" to the
  /// person: say why it was closed instead.
  private func transportFailure(_ error: Error) -> Error {
    guard let url = error as? URLError, url.code == .cancelled else { return error }
    return terminalFailure ?? ClientFailure.disconnected
  }

  func next() async throws -> Frame {
    let frame = try await readFrame()
    greetingDeadline?.cancel()
    greetingDeadline = nil
    return frame
  }

  private func readFrame() async throws -> Frame {
    if let webSocket {
      let message: URLSessionWebSocketTask.Message
      do { message = try await webSocket.receive() } catch { throw transportFailure(error) }
      switch message {
      case .string(let text):
        guard !text.isEmpty, text.utf8.count <= ProtocolLimits.jsonBytes else {
          throw ClientFailure.invalidFrame
        }
        return Frame(kind: FrameKind.json, payload: Data(text.utf8))
      case .data(let bytes):
        let frame = try Frame.decode(bytes)
        guard frame.kind != FrameKind.json else { throw ClientFailure.invalidFrame }
        return frame
      @unknown default: throw ClientFailure.invalidFrame
      }
    }
    let prefix = try await exact(5)
    let size = prefix.dropFirst().reduce(0) { ($0 << 8) | Int($1) }
    // Validate before allocating or reading the advertised body.
    guard let kind = prefix.first, Frame.accepts(kind: kind, length: size) else {
      throw ClientFailure.invalidFrame
    }
    return try Frame.decode(prefix + (try await exact(size)))
  }
}

/// What one unit puts on a transport: a WebSocket text message for the JSON frame and a binary
/// message for each other frame, or, on the local socket, every frame in one write.
enum Outgoing: Sendable, Equatable {
  case text(String)
  case binary(Data)

  static func unit(_ frames: [Frame], singleWrite: Bool) throws -> [Outgoing] {
    if singleWrite {
      var data = Data()
      for frame in frames { data.append(try frame.encode()) }
      return [.binary(data)]
    }
    return try frames.map { frame in
      frame.kind == FrameKind.json
        ? .text(String(decoding: frame.payload, as: UTF8.self)) : .binary(try frame.encode())
    }
  }
}

struct WirePayload {
  let message: JsonValue
  let attachments: [WebHostAttachment]

  func frames(maxAttachments: Int = ProtocolLimits.promptAttachments) throws -> [Frame] {
    guard attachments.count <= maxAttachments else { throw ClientFailure.invalidFrame }
    var frames: [Frame] = []
    for attachment in attachments {
      guard !attachment.bytes.isEmpty,
        attachment.reference.bytes == String(attachment.bytes.count),
        attachment.bytes.count
          <= max(ProtocolLimits.promptAttachmentBytes, ProtocolLimits.webScreenshotBytes),
        let uuid = UUID(uuidString: attachment.reference.attachmentId),
        uuid.uuidString.lowercased() == attachment.reference.attachmentId
      else { throw ClientFailure.invalidFrame }
      let header = withUnsafeBytes(of: uuid.uuid) { Data($0) }
      var offset = 0
      repeat {
        let end = min(offset + ProtocolLimits.attachmentPayloadBytes, attachment.bytes.count)
        frames.append(
          Frame(
            kind: FrameKind.attachment, header: header,
            payload: attachment.bytes.subdata(in: offset..<end)))
        offset = end
      } while offset < attachment.bytes.count
    }
    let json = try JSONEncoder().encode(message)
    guard json.count <= ProtocolLimits.jsonBytes else { throw ClientFailure.invalidFrame }
    frames.append(Frame(kind: FrameKind.json, payload: json))
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

/// A frame the reader has taken in. A JSON frame is parsed to a tree once, here, away from the
/// main actor; the reader waits for the parse before it reads the next frame, so frames are
/// applied in the order they arrived. What the tree holds is read into types on the main actor,
/// without writing it out as text again (`JsonValue.decoded`).
enum IncomingFrame: Sendable {
  case message(JsonValue)
  case other(Frame)

  /// The tree of a JSON frame, parsed on a background executor.
  @concurrent static func parse(_ frame: Frame) async throws -> IncomingFrame {
    .message(try JSONDecoder().decode(JsonValue.self, from: frame.payload))
  }
}
