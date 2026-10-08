import SoyehtProtocol
import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
  import AppKit
#elseif os(iOS)
  import PhotosUI
  import UIKit
#endif

/// Sizes the pane interior does not already name. The views are flat, palette-only
/// and thin: the visual redesign replaces them, and the state and the staging stay
/// in `AgentComposerAttachments`.
private enum AttachmentMetrics {
  static let thumbnail: CGFloat = 26
  static let thumbnailRadius: CGFloat = 4
  static let chipNameWidth: CGFloat = 160
}

/// Attachment chips: removable in the composer, read-only in a sent message.
struct AgentAttachmentChips: View {
  let items: [AgentAttachmentPresentation]
  let style: AgentConversationStyle
  var remove: ((String) -> Void)?

  var body: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: PaneInteriorMetrics.compactSpacing) {
        ForEach(items) { item in chip(item) }
      }
    }
  }

  private func chip(_ item: AgentAttachmentPresentation) -> some View {
    HStack(spacing: PaneInteriorMetrics.compactSpacing) {
      preview(item)
      VStack(alignment: .leading, spacing: 0) {
        Text(item.name).lineLimit(1).truncationMode(.middle).foregroundStyle(style.ink)
        Text(item.detail).foregroundStyle(style.muted)
      }.frame(maxWidth: AttachmentMetrics.chipNameWidth, alignment: .leading)
      if let remove {
        Button {
          remove(item.id)
        } label: {
          Image(systemName: "xmark.circle.fill").foregroundStyle(style.muted)
        }.buttonStyle(.plain).accessibilityLabel("Remove \(item.name)")
      }
    }
    .font(style.captionFont)
    .padding(.horizontal, PaneInteriorMetrics.compactSpacing)
    .padding(.vertical, PaneInteriorMetrics.tightSpacing)
    .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
    .accessibilityElement(children: .combine)
  }

  @ViewBuilder private func preview(_ item: AgentAttachmentPresentation) -> some View {
    if let image = item.thumbnail.flatMap(Self.image) {
      image.resizable().scaledToFill()
        .frame(width: AttachmentMetrics.thumbnail, height: AttachmentMetrics.thumbnail)
        .clipShape(RoundedRectangle(cornerRadius: AttachmentMetrics.thumbnailRadius))
    } else {
      Image(systemName: item.systemImage).foregroundStyle(style.secondary)
        .frame(width: AttachmentMetrics.thumbnail, height: AttachmentMetrics.thumbnail)
    }
  }

  private static func image(_ data: Data) -> Image? {
    #if os(macOS)
      NSImage(data: data).map(Image.init(nsImage:))
    #else
      UIImage(data: data).map(Image.init(uiImage:))
    #endif
  }
}

/// The composer's attachments row: chips and the latest refusal.
struct AgentComposerAttachmentRow: View {
  let attachments: AgentComposerAttachments
  let style: AgentConversationStyle

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      if !attachments.items.isEmpty {
        AgentAttachmentChips(
          items: attachments.items.map(AgentAttachmentPresentation.init(draft:)), style: style,
          remove: { id in UUID(uuidString: id).map(attachments.remove) })
      }
      if let notice = attachments.notice {
        Label(notice, systemImage: "exclamationmark.triangle").font(style.captionFont)
          .foregroundStyle(style.warning)
      }
    }
  }
}

/// The composer's "+": a file panel on the Mac; on iPhone and iPad a menu with
/// the photo library, Files and, when the pasteboard holds one, a pasted file or image.
struct AgentAttachButton: View {
  let attachments: AgentComposerAttachments
  let style: AgentConversationStyle
  @State private var importing = false
  #if os(iOS)
    @State private var pickingPhotos = false
    @State private var photos: [PhotosPickerItem] = []
  #endif

  var body: some View {
    control
      .fileImporter(
        isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true
      ) { result in
        if case .success(let urls) = result { attachments.attach(urls: urls) }
      }
  }

  private var icon: some View {
    Image(systemName: "plus")
      .font(.system(size: AgentComposerMetrics.iconSize - 2, weight: .medium))
      .foregroundStyle(style.secondary)
      .frame(width: AgentComposerMetrics.controlHeight, height: AgentComposerMetrics.controlHeight)
      .contentShape(Circle())
      .accessibilityLabel("Attach files")
  }

  #if os(macOS)
    private var control: some View {
      Button {
        importing = true
      } label: {
        icon
      }.buttonStyle(.plain).help("Attach files")
    }
  #else
    private var control: some View {
      Menu {
        Button("Photo Library", systemImage: "photo.on.rectangle") { pickingPhotos = true }
        Button("Files", systemImage: "folder") { importing = true }
        if AgentPasteboardContent.hasAttachableContent(.general) {
          Button("Paste", systemImage: "doc.on.clipboard") {
            attachments.paste(AgentPasteboardContent(pasteboard: .general))
          }
        }
      } label: {
        icon
      }
      .photosPicker(
        isPresented: $pickingPhotos, selection: $photos,
        maxSelectionCount: ProtocolLimits.promptAttachments, matching: .images
      )
      .onChange(of: photos) { _, picked in
        guard !picked.isEmpty else { return }
        photos = []
        Task {
          for item in picked {
            guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
            let type = item.supportedContentTypes.first { $0.conforms(to: .image) } ?? .jpeg
            attachments.attach(data: data, type: type, name: nil)
          }
        }
      }
    }
  #endif
}

/// Files and images dropped anywhere on the conversation attach to the composer.
struct AgentAttachmentDropTarget: ViewModifier {
  let attachments: AgentComposerAttachments
  let style: AgentConversationStyle
  @State private var targeted = false

  func body(content: Content) -> some View {
    content
      .onDrop(of: AgentComposerAttachments.droppedTypes, isTargeted: $targeted) { providers in
        attachments.attach(providers: providers)
      }
      .overlay {
        if targeted {
          RoundedRectangle(cornerRadius: PaneInteriorMetrics.cardRadius)
            .strokeBorder(style.accent, lineWidth: PaneInteriorMetrics.separatorWidth * 2)
            .allowsHitTesting(false)
        }
      }
  }
}

#if os(macOS)
  /// Command-V in the focused composer. Copied files and images become
  /// attachments; text is left to the text field. A plain text field cannot take
  /// an image, so the key is read before AppKit hands it to the field editor.
  struct AgentComposerPaste: ViewModifier {
    let isFocused: Bool
    let attachments: AgentComposerAttachments
    @State private var window = WindowReference()
    @State private var monitor: Any?

    func body(content: Content) -> some View {
      content
        .background(HostWindowReader(reference: window))
        .onChange(of: isFocused, initial: true) { _, focused in
          focused ? install() : uninstall()
        }
        .onDisappear(perform: uninstall)
    }

    static func isPasteKey(characters: String?, modifiers: NSEvent.ModifierFlags) -> Bool {
      characters?.lowercased() == "v"
        && modifiers.intersection([.command, .shift, .option, .control]) == .command
    }

    private func install() {
      guard monitor == nil else { return }
      let window = window
      let attachments = attachments
      monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
        guard
          Self.isPasteKey(
            characters: event.charactersIgnoringModifiers, modifiers: event.modifierFlags),
          let source = event.window.map(ObjectIdentifier.init)
        else { return event }
        let consumed = MainActor.assumeIsolated {
          source == window.window.map(ObjectIdentifier.init)
            && attachments.paste(AgentPasteboardContent(pasteboard: .general))
        }
        return consumed ? nil : event
      }
    }

    private func uninstall() {
      if let monitor { NSEvent.removeMonitor(monitor) }
      monitor = nil
    }
  }

  @MainActor final class WindowReference {
    weak var window: NSWindow?
  }

  /// Reports the window hosting the composer, so its key monitor ignores other windows.
  private struct HostWindowReader: NSViewRepresentable {
    let reference: WindowReference

    func makeNSView(context: Context) -> NSView { WindowTrackingView(reference: reference) }
    func updateNSView(_ view: NSView, context: Context) {}

    private final class WindowTrackingView: NSView {
      let reference: WindowReference
      init(reference: WindowReference) {
        self.reference = reference
        super.init(frame: .zero)
      }
      required init?(coder: NSCoder) { nil }
      override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        reference.window = window
      }
    }
  }
#endif
