import Foundation
import Observation
import UniformTypeIdentifiers

/// The composer's attachments: files from the attach button, drops and pastes,
/// staged off the main actor and kept within the engine's limits. A refused
/// file leaves a notice instead of the chip.
@MainActor @Observable public final class AgentComposerAttachments {
  public private(set) var list = AgentAttachmentList()
  public private(set) var notice: String?
  /// True on the engine's Mac, where a non-image file is linked in place.
  let linkHostFiles: Bool

  public init(linkHostFiles: Bool) {
    self.linkHostFiles = linkHostFiles
  }

  /// What a drop may carry: files, images and PDFs. Not plain data or text, so
  /// the pane, editor-tab and task drags keep reaching their own targets.
  public static let droppedTypes: [UTType] = [.fileURL, .image, .pdf]

  public var items: [AgentAttachmentDraft] { list.items }

  public func remove(_ id: UUID) {
    list.remove(id)
    notice = nil
  }

  /// Hands the drafts to the prompt being sent and empties the composer.
  public func take() -> [AgentAttachmentDraft] {
    defer {
      list.removeAll()
      notice = nil
    }
    return list.items
  }

  public func add(_ results: [AgentAttachmentResult]) {
    notice = nil
    for result in results {
      do {
        try list.add(result.get())
      } catch {
        notice = error.message
      }
    }
  }

  public func attach(urls: [URL]) {
    let link = linkHostFiles
    stage {
      urls.map { url in
        Result { () throws(AgentAttachmentRejection) in
          try AgentAttachmentStaging.file(at: url, linkHostFiles: link)
        }
      }
    }
  }

  public func attach(data: Data, type: UTType, name: String?) {
    stage {
      if AgentAttachmentStaging.kind(of: type) == .image {
        guard let draft = AgentAttachmentStaging.image(data, type: type, name: name) else {
          return [.failure(.unreadable(name: name ?? AgentAttachmentStaging.fallbackImageName))]
        }
        return [.success(draft)]
      }
      return [.success(AgentAttachmentStaging.bytes(data, name: name ?? "Attachment", type: type))]
    }
  }

  /// Attaches a paste of files or an image; false leaves it to the text view.
  @discardableResult public func paste(_ content: AgentPasteboardContent) -> Bool {
    guard content.offersAttachments else { return false }
    let link = linkHostFiles
    stage { content.drafts(linkHostFiles: link) }
    return true
  }

  /// Accepts dropped files and images; false when nothing in the drop is attachable.
  public func attach(providers: [NSItemProvider]) -> Bool {
    let attachable = providers.filter { provider in
      Self.droppedTypes.contains { type in
        provider.hasItemConformingToTypeIdentifier(type.identifier)
      }
    }
    guard !attachable.isEmpty else { return false }
    Task {
      for provider in attachable {
        await load(provider)
      }
    }
    return true
  }

  private func load(_ provider: NSItemProvider) async {
    if provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
      guard let data = await Self.data(provider, type: .fileURL),
        let url = URL(dataRepresentation: data, relativeTo: nil)
      else { return add([.failure(.unreadable(name: provider.suggestedName ?? "File"))]) }
      return attach(urls: [url])
    }
    let type =
      provider.registeredContentTypes.first { type in
        Self.droppedTypes.contains { type.conforms(to: $0) }
      } ?? .data
    guard let data = await Self.data(provider, type: type) else {
      return add([.failure(.unreadable(name: provider.suggestedName ?? "File"))])
    }
    attach(data: data, type: type, name: provider.suggestedName)
  }

  private static func data(_ provider: NSItemProvider, type: UTType) async -> Data? {
    await withCheckedContinuation { continuation in
      _ = provider.loadDataRepresentation(for: type) { data, _ in
        continuation.resume(returning: data)
      }
    }
  }

  /// Reads and encodes off the main actor, then adds the result in order.
  private func stage(
    _ work: @escaping @Sendable () -> [AgentAttachmentResult]
  ) {
    Task {
      let results = await Task.detached(priority: .userInitiated) { work() }.value
      add(results)
    }
  }
}
