import Foundation
import Network
import SoyehtProtocol

public enum ClientFailure: Error, LocalizedError, Sendable {
  case disconnected, invalidFrame, invalidSnapshot, invalidStream, timeout
  case remote(String, String)
  var canRetry: Bool {
    switch self {
    case .disconnected, .timeout: true
    default: false
    }
  }
  public var errorDescription: String? {
    switch self {
    case .disconnected: "Conexão interrompida. Reconectando…"
    case .invalidFrame: "O engine enviou uma mensagem inválida."
    case .invalidSnapshot: "O estado recebido está incompleto."
    case .invalidStream: "A tela precisa ser sincronizada novamente."
    case .timeout: "O engine não respondeu a tempo."
    case .remote(let kind, let message): "\(message) (\(kind))"
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
  private let connection: NWConnection?
  private var webSocket: URLSessionWebSocketTask?
  private var session: URLSession?
  private var greetingDeadline: Task<Void, Never>?
  private var openWaiter: CheckedContinuation<Void, Error>?
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
            self.openWaiter?.resume()
            self.openWaiter = nil
          case .failed(let error), .waiting(let error):
            self.openWaiter?.resume(throwing: error)
            self.openWaiter = nil
          case .cancelled:
            self.openWaiter?.resume(throwing: ClientFailure.disconnected)
            self.openWaiter = nil
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
    openWaiter?.resume(throwing: ClientFailure.disconnected)
    openWaiter = nil
  }

  func send(_ value: JsonValue) async throws {
    guard !closed else { throw ClientFailure.disconnected }
    let json = try JSONEncoder().encode(value)
    guard json.count <= 1_048_576 else { throw ClientFailure.invalidFrame }
    if let webSocket {
      try await webSocket.send(.string(String(decoding: json, as: UTF8.self)))
      return
    }
    guard let connection else { throw ClientFailure.disconnected }
    let data = try Frame(kind: 1, payload: json).encode()
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
      guard !closed else { throw ClientFailure.disconnected }
      let remaining = count - result.count
      let chunk: Data = try await withCheckedThrowingContinuation { continuation in
        connection.receive(minimumIncompleteLength: 1, maximumLength: remaining) {
          data, _, _, error in
          guard error == nil, let data, !data.isEmpty else {
            continuation.resume(throwing: ClientFailure.disconnected)
            return
          }
          continuation.resume(returning: data)
        }
      }
      result.append(chunk)
    }
    return result
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
