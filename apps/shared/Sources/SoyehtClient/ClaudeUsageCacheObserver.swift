#if os(macOS)
  import Darwin
  import Foundation

  @MainActor
  public final class ClaudeUsageCacheObserver {
    private struct Signature: Equatable {
      let modifiedAt: TimeInterval
      let byteCount: UInt64
    }

    private let cacheURL: URL
    private var source: DispatchSourceFileSystemObject?
    private var lastSignature: Signature?
    private var pendingRefresh: Task<Void, Never>?
    private var onChange: (@MainActor @Sendable () -> Void)?

    public init(
      cacheURL: URL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude.json")
    ) {
      self.cacheURL = cacheURL
    }

    public func start(onChange: @escaping @MainActor @Sendable () -> Void) {
      guard source == nil else { return }
      self.onChange = onChange
      lastSignature = signature()
      let descriptor = Darwin.open(cacheURL.deletingLastPathComponent().path, O_EVTONLY)
      guard descriptor >= 0 else { return }
      let source = DispatchSource.makeFileSystemObjectSource(
        fileDescriptor: descriptor,
        eventMask: [.write, .rename, .delete],
        // The event handler is MainActor-isolated; its dispatch queue must match.
        queue: .main)
      source.setEventHandler { [weak self] in self?.directoryChanged() }
      source.setCancelHandler { _ = Darwin.close(descriptor) }
      self.source = source
      source.resume()
    }

    public func stop() {
      pendingRefresh?.cancel()
      pendingRefresh = nil
      source?.cancel()
      source = nil
      onChange = nil
    }

    private func directoryChanged() {
      let current = signature()
      guard current != lastSignature else { return }
      lastSignature = current
      pendingRefresh?.cancel()
      pendingRefresh = Task { [weak self] in
        try? await Task.sleep(for: .milliseconds(200))
        guard !Task.isCancelled else { return }
        self?.onChange?()
      }
    }

    private func signature() -> Signature? {
      guard
        let attributes = try? FileManager.default.attributesOfItem(atPath: cacheURL.path),
        let modifiedAt = attributes[.modificationDate] as? Date,
        let byteCount = attributes[.size] as? NSNumber
      else { return nil }
      return Signature(
        modifiedAt: modifiedAt.timeIntervalSince1970,
        byteCount: byteCount.uint64Value)
    }

    deinit {
      source?.cancel()
    }
  }
#endif
