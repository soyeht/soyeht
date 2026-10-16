import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// A document image, fetched from the engine when it appears.
struct WritingAssetImage: View {
  @Bindable var client: EngineClient
  let fileId: String
  let asset: String
  @State private var image: NSImage?

  var body: some View {
    Group {
      if let image {
        Image(nsImage: image).resizable().scaledToFit()
      } else {
        ProgressView().controlSize(.small)
      }
    }
    .task(id: "\(fileId):\(asset)") {
      do {
        let bytes = try await client.readWritingAsset(fileId: fileId, asset: asset)
        guard let decoded = NSImage(data: bytes) else { throw ClientFailure.invalidFrame }
        image = decoded
      } catch { client.problem = error.localizedDescription }
    }
  }
}

/// The text view of a writing surface: Command-B and Command-I format the selection, and a
/// pasted or dropped image goes to `imageHandler` first.
@MainActor final class WritingImagePasteTextView: NSTextView {
  var imageHandler: ((NSPasteboard) -> Bool)?
  var writingFormatShortcutHandler: ((WritingFormatKind) -> Void)?

  /// Accepts images pasted or dropped on the view; `insert` returns whether it took them.
  func acceptImages(_ insert: @escaping (NSPasteboard) -> Bool) {
    registerForDraggedTypes([.png, .tiff, .fileURL])
    imageHandler = insert
  }

  override func performKeyEquivalent(with event: NSEvent) -> Bool {
    let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
    let conflictingModifiers: NSEvent.ModifierFlags = [.control, .option, .shift]
    guard flags.contains(.command), flags.intersection(conflictingModifiers).isEmpty,
      let kind = WritingChromePolicy.formatShortcut(
        key: event.charactersIgnoringModifiers ?? "", command: true),
      let writingFormatShortcutHandler
    else { return super.performKeyEquivalent(with: event) }
    writingFormatShortcutHandler(kind)
    return true
  }

  override func paste(_ sender: Any?) {
    if imageHandler?(NSPasteboard.general) == true { return }
    super.paste(sender)
  }

  override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
    if imageHandler?(sender.draggingPasteboard) == true { return true }
    return super.performDragOperation(sender)
  }
}

enum WritingImageInsertion {
  /// Inserts the image on `pasteboard` at the selection of `textView`. Returns false when the
  /// pasteboard holds no image, so the view can paste it as text.
  @MainActor static func insert(
    from pasteboard: NSPasteboard, in textView: NSTextView, text: WritingViewportText,
    client: EngineClient, fileId: String
  ) -> Bool {
    let selection = textView.selectedRange()
    guard let start = text.position(utf16Offset: selection.location),
      let end = text.position(utf16Offset: NSMaxRange(selection)),
      let image = NSImage(pasteboard: pasteboard)
    else { return false }
    let attachment: WebHostAttachment
    do { attachment = try WritingImageAttachment.make(from: image) } catch {
      client.problem = error.localizedDescription
      return true
    }
    let range = SoyehtProtocol.TextRange(start: start, end: end)
    client.perform {
      _ = try await client.formatWritingDocument(
        fileId: fileId, range: range, kind: .image, image: attachment)
    }
    return true
  }
}

enum WritingImageAttachment {
  static func make(from image: NSImage) throws -> WebHostAttachment {
    let bytes = try image.boundedPNG(
      maxBytes: ProtocolLimits.writingImageBytes,
      failures: BoundedPNGFailures(
        encoding: ClientFailure.editorCommand("The image could not be encoded."),
        resizing: ClientFailure.editorCommand("The image could not be resized."),
        limit: ClientFailure.editorCommand("The image exceeds its byte limit.")))
    let reference = AttachmentRef(
      attachmentId: UUID().uuidString.lowercased(), mimeType: "image/png", name: "image.png",
      bytes: String(bytes.count))
    return WebHostAttachment(reference: reference, bytes: bytes)
  }
}
