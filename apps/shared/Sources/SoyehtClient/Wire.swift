import Foundation
import Network
import SoyehtProtocol
import os

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

/// Owns one connection. All reads and writes are asynchronous and none of it runs on the main
/// actor: the connection's callbacks resume the waiting reader or writer directly, so a main
/// thread busy for seconds delays nothing that is read from or written to the engine (ADR 0171).
/// The state is behind a lock, never held across a suspension.
final class Wire: Sendable {
  private struct ReadWaiter {
    let id: UInt64
    let continuation: CheckedContinuation<Data, Error>
  }

  private struct State {
    var greetingDeadline: Task<Void, Never>?
    var openWaiter: CheckedContinuation<Void, Error>?
    var readWaiter: ReadWaiter?
    var nextReadId: UInt64 = 0
    var terminalFailure: Error?
    var opened = false
    var closed = false
    /// Bytes read from the local socket and not yet framed, from `consumed` on.
    var buffer = Data()
    var consumed = 0
    var sendQueue: SendQueue<[Outgoing]>?
  }

  /// The most one read takes off the local socket: several small frames arrive in one read.
  private static let readBytes = 256 * 1024

  private let connection: NWConnection?
  private let webSocket: URLSessionWebSocketTask?
  private let session: URLSession?
  private let state = OSAllocatedUnfairLock(initialState: State())

  init(socket: String) {
    connection = NWConnection(to: .unix(path: socket), using: .tcp)
    webSocket = nil
    session = nil
  }

  init(url: URL) {
    connection = nil
    let session = URLSession(configuration: .ephemeral)
    self.session = session
    let task = session.webSocketTask(with: url)
    task.maximumMessageSize = ProtocolLimits.jsonBytes
    webSocket = task
  }

  /// A unit that fails on the transport leaves the engine with frames it never saw committed, so
  /// the connection ends with it.
  private var sendQueue: SendQueue<[Outgoing]> {
    state.withLock { state in
      if let queue = state.sendQueue { return queue }
      let queue = SendQueue<[Outgoing]>(
        transmit: { [weak self] unit in
          guard let self else { throw ClientFailure.disconnected }
          try await self.transmit(unit)
        },
        onFailure: { [weak self] _ in self?.close() })
      state.sendQueue = queue
      return queue
    }
  }

  func open(greetingTimeout: Duration = greetingTimeout) async throws {
    let deadline = Task { [weak self] in
      do { try await Task.sleep(for: greetingTimeout) } catch { return }
      self?.close(ClientFailure.timeout)
    }
    state.withLock { $0.greetingDeadline = deadline }
    if let webSocket {
      webSocket.resume()
      return
    }
    guard let connection else { throw ClientFailure.disconnected }
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
      let failure = state.withLock { state -> Error? in
        if let failure = state.terminalFailure { return failure }
        state.openWaiter = continuation
        return nil
      }
      if let failure {
        continuation.resume(throwing: failure)
        return
      }
      connection.stateUpdateHandler = { [weak self] state in self?.connectionChanged(state) }
      connection.start(queue: DispatchQueue(label: "com.soyeht.next.dev.client.socket"))
    }
  }

  private func connectionChanged(_ change: NWConnection.State) {
    switch change {
    case .ready:
      let waiter = state.withLock { state -> CheckedContinuation<Void, Error>? in
        state.opened = true
        defer { state.openWaiter = nil }
        return state.openWaiter
      }
      waiter?.resume()
    case .failed(let error), .waiting(let error):
      fail(state.withLock { $0.opened } ? ClientFailure.disconnected : error)
    case .cancelled:
      fail(ClientFailure.disconnected)
    default: break
    }
  }

  /// Ends the connection. `reason` is what a pending or later read reports, so a deadline says
  /// "no answer in time" instead of the transport's own "cancelled".
  func close(_ reason: ClientFailure = .disconnected) {
    let (queue, deadline) = state.withLock { state in
      state.closed = true
      defer { state.greetingDeadline = nil }
      return (state.sendQueue, state.greetingDeadline)
    }
    fail(reason)
    if let queue { Task { await queue.fail(reason) } }
    connection?.cancel()
    webSocket?.cancel(with: .goingAway, reason: nil)
    session?.invalidateAndCancel()
    deadline?.cancel()
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
    guard isOpen else { throw ClientFailure.disconnected }
    try await sendQueue.send(try Outgoing.unit(frames, singleWrite: webSocket == nil))
  }

  var isOpen: Bool { state.withLock { !$0.closed && $0.terminalFailure == nil } }

  private func transmit(_ unit: [Outgoing]) async throws {
    guard isOpen else { throw ClientFailure.disconnected }
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

  /// The next chunk of the local socket, at least one byte.
  private func receiveChunk() async throws -> Data {
    guard let connection else { throw ClientFailure.disconnected }
    return try await withCheckedThrowingContinuation { continuation in
      let id = state.withLock { state -> UInt64? in
        guard !state.closed, state.terminalFailure == nil else { return nil }
        state.nextReadId &+= 1
        state.readWaiter = ReadWaiter(id: state.nextReadId, continuation: continuation)
        return state.nextReadId
      }
      guard let id else {
        continuation.resume(throwing: ClientFailure.disconnected)
        return
      }
      connection.receive(minimumIncompleteLength: 1, maximumLength: Self.readBytes) {
        [weak self] data, _, _, error in
        self?.finishRead(id, data: data, error: error)
      }
    }
  }

  private func finishRead(_ id: UInt64, data: Data?, error: Error?) {
    let waiter = state.withLock { state -> ReadWaiter? in
      guard state.readWaiter?.id == id else { return nil }
      defer { state.readWaiter = nil }
      return state.readWaiter
    }
    guard let waiter else { return }
    guard error == nil, let data, !data.isEmpty else {
      waiter.continuation.resume(throwing: ClientFailure.disconnected)
      return
    }
    waiter.continuation.resume(returning: data)
  }

  private func fail(_ error: Error) {
    let (open, read) = state.withLock { state in
      if state.terminalFailure == nil { state.terminalFailure = error }
      defer { (state.openWaiter, state.readWaiter) = (nil, nil) }
      return (state.openWaiter, state.readWaiter)
    }
    open?.resume(throwing: error)
    read?.continuation.resume(throwing: ClientFailure.disconnected)
  }

  /// A socket this wire closed reports `URLError.cancelled`, which reads "cancelled" to the
  /// person: say why it was closed instead.
  private func transportFailure(_ error: Error) -> Error {
    guard let url = error as? URLError, url.code == .cancelled else { return error }
    return state.withLock { $0.terminalFailure } ?? ClientFailure.disconnected
  }

  func next() async throws -> Frame {
    let frame = try await readFrame()
    let deadline = state.withLock { state in
      defer { state.greetingDeadline = nil }
      return state.greetingDeadline
    }
    deadline?.cancel()
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
    while true {
      guard isOpen else { throw ClientFailure.disconnected }
      if let frame = try bufferedFrame() { return frame }
      let chunk = try await receiveChunk()
      state.withLock { $0.buffer.append(chunk) }
    }
  }

  /// The next whole frame of the bytes read so far, if they hold one.
  private func bufferedFrame() throws -> Frame? {
    let bytes: Data? = try state.withLock { state in
      let available = state.buffer.count - state.consumed
      guard available >= 5 else { return nil }
      let start = state.consumed
      let size = (1...4).reduce(0) { ($0 << 8) | Int(state.buffer[start + $1]) }
      // Validate before waiting for the advertised body.
      guard Frame.accepts(kind: state.buffer[start], length: size) else {
        throw ClientFailure.invalidFrame
      }
      guard available >= 5 + size else { return nil }
      let whole = state.buffer.subdata(in: start..<(start + 5 + size))
      state.consumed += 5 + size
      if state.consumed == state.buffer.count {
        state.buffer.removeAll(keepingCapacity: true)
        state.consumed = 0
      } else if state.consumed >= Self.readBytes {
        state.buffer.removeSubrange(0..<state.consumed)
        state.consumed = 0
      }
      return whole
    }
    return try bytes.map(Frame.decode)
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
