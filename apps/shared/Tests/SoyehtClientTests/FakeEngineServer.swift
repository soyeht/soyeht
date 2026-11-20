#if os(macOS)
  import Darwin
  import Foundation
  import SoyehtProtocol

  @testable import SoyehtClient

  @MainActor private func busySlice() { Thread.sleep(forTimeInterval: 0.05) }

  /// The main actor kept busy for `seconds`, the way a window of many panes keeps it: work in
  /// slices of 50 ms with the other main-actor jobs between them, so each hop a connection takes
  /// through the main actor costs a slice. (A hold with no turns at all would also stop every
  /// other test of the suite, which share the main actor.)
  @MainActor func keepMainActorBusy(for seconds: Double) async {
    let end = DispatchTime.now().uptimeNanoseconds + UInt64(seconds * 1_000_000_000)
    while DispatchTime.now().uptimeNanoseconds < end {
      busySlice()
      await Task.yield()
    }
  }

  /// An engine that speaks only the frames: it greets, records every request it reads with the
  /// time it read it, answers each through `answer`, and sends whatever a test pushes. It runs on
  /// its own threads, so a test that blocks the main actor does not block it.
  final class FakeEngineServer: @unchecked Sendable {
    struct Request: Sendable {
      let id: String
      let method: String
      let params: JsonValue
      /// `DispatchTime` uptime, the clock of the test's own readings.
      let arrivedNs: UInt64
    }

    /// The answer's `result`, or `nil` to leave the request unanswered.
    typealias Answer = @Sendable (Request) -> JsonValue?

    let path = "/tmp/sn-fe-\(UUID().uuidString.prefix(8)).sock"
    private let listener: Int32
    private let lock = NSLock()
    private var connection: Int32 = -1
    private var received: [Request] = []
    private var answer: Answer

    init(answer: @escaping Answer = { _ in .object([:]) }) throws {
      self.answer = answer
      listener = socket(AF_UNIX, SOCK_STREAM, 0)
      var address = sockaddr_un()
      address.sun_family = sa_family_t(AF_UNIX)
      withUnsafeMutableBytes(of: &address.sun_path) { buffer in
        _ = path.withCString {
          strlcpy(buffer.baseAddress!.assumingMemoryBound(to: CChar.self), $0, buffer.count)
        }
      }
      let bound = withUnsafePointer(to: &address) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
          bind(listener, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
      }
      guard bound == 0, listen(listener, 1) == 0 else { throw ClientFailure.disconnected }
      Thread.detachNewThread { [self] in serve() }
    }

    var requests: [Request] { lock.withLock { received } }

    func setAnswer(_ answer: @escaping Answer) { lock.withLock { self.answer = answer } }

    /// Frames, sent to the client in this order and in as many writes as `slices` says.
    func push(_ frames: [Frame], slices: Int = 1) throws {
      var bytes = Data()
      for frame in frames { bytes.append(try frame.encode()) }
      let step = max(1, (bytes.count + slices - 1) / slices)
      var offset = 0
      while offset < bytes.count {
        write(bytes.subdata(in: offset..<min(offset + step, bytes.count)))
        offset += step
      }
    }

    func push(json values: [JsonValue]) throws {
      try push(
        try values.map { Frame(kind: FrameKind.json, payload: try JSONEncoder().encode($0)) })
    }

    func stop() {
      let fd = lock.withLock { connection }
      if fd >= 0 { close(fd) }
      close(listener)
      unlink(path)
    }

    private func write(_ data: Data) {
      let fd = lock.withLock { connection }
      guard fd >= 0 else { return }
      lock.withLock {
        data.withUnsafeBytes { buffer in
          var sent = 0
          while sent < buffer.count {
            let count = Darwin.write(fd, buffer.baseAddress! + sent, buffer.count - sent)
            if count <= 0 { return }
            sent += count
          }
        }
      }
    }

    private func serve() {
      let fd = accept(listener, nil, nil)
      guard fd >= 0 else { return }
      lock.withLock { connection = fd }
      try? push(json: [.object(["jsonrpc": .string("2.0"), "method": .string("auth.challenge")])])
      while let header = read(fd, count: 5) {
        let size = header.dropFirst().reduce(0) { ($0 << 8) | Int($1) }
        guard let body = read(fd, count: size),
          let frame = try? Frame.decode(header + body)
        else { return }
        guard frame.kind == FrameKind.json,
          let message = try? JSONDecoder().decode(JsonValue.self, from: frame.payload),
          let id = message["id"].string, let method = message["method"].string
        else { continue }
        let request = Request(
          id: id, method: method, params: message["params"],
          arrivedNs: DispatchTime.now().uptimeNanoseconds)
        let respond = lock.withLock {
          received.append(request)
          return answer
        }
        guard let result = respond(request) else { continue }
        try? push(json: [
          .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": result])
        ])
      }
    }

    private func read(_ fd: Int32, count: Int) -> Data? {
      var data = Data(count: count)
      var done = 0
      while done < count {
        let n = data.withUnsafeMutableBytes {
          Darwin.read(fd, $0.baseAddress! + done, count - done)
        }
        if n <= 0 { return nil }
        done += n
      }
      return data
    }
  }
#endif
