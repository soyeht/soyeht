import Foundation
import SoyehtProtocol
import SwiftUI

#if os(macOS)
  import AppKit
#endif

/// One transcript entry in the approved flat interior: a person's or a peer agent's message is
/// a `canvas` card, the agent's reply is plain text, and a tool call is a plain row over a thin
/// `border` line.
struct AgentTranscriptRow: View {
  let message: TranscriptMessage
  let workspaceId: String
  let style: AgentConversationStyle
  @State private var showsThought = false

  var body: some View {
    Group {
      switch message.kind {
      case .handover:
        Label {
          Text("Conversation continued with \(message.text)")
        } icon: {
          Image(systemName: "arrow.triangle.2.circlepath")
        }
        .font(style.captionFont).foregroundStyle(style.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
      case .thought:
        DisclosureGroup("Thought", isExpanded: $showsThought) {
          FilePathText(message.text, workspaceId: workspaceId)
            .textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, PaneInteriorMetrics.compactSpacing)
        }.foregroundStyle(style.secondary)
      case .tool:
        if let tool = message.tool {
          AgentToolRow(tool: tool, workspaceId: workspaceId, style: style)
        }
      case .terminal:
        if let output = message.terminal {
          FilePathText(
            Data(base64Encoded: output.bytes).map { String(decoding: $0, as: UTF8.self) } ?? "",
            workspaceId: workspaceId
          )
          .font(style.monoFont).textSelection(.enabled)
          .frame(maxWidth: .infinity, alignment: .leading)
          .padding(PaneInteriorMetrics.rowSpacing)
          .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
        }
      case .plan:
        VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
          Label("Plan", systemImage: "list.bullet.clipboard").fontWeight(.semibold)
          ForEach(Array((message.plan ?? []).enumerated()), id: \.offset) { _, entry in
            Label(
              entry.content,
              systemImage: entry.status == .completed ? "checkmark.circle.fill" : "circle")
          }
        }.font(style.captionFont).frame(maxWidth: .infinity, alignment: .leading)
      case .usage:
        EmptyView()
      default:
        if message.role == .user || message.agentMessage != nil {
          addressedMessage
        } else {
          FilePathText(message.text, workspaceId: workspaceId)
            .textSelection(.enabled)
            .foregroundStyle(message.role == .system ? style.secondary : style.ink)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
      }
    }.font(style.bodyFont)
  }

  /// A message someone sent to this agent: the person ("YOU") or a peer agent by name.
  private var addressedMessage: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      Text(message.agentMessage?.displayName ?? "You")
        .font(style.captionFont.weight(.semibold)).textCase(.uppercase)
        .foregroundStyle(message.agentMessage == nil ? style.secondary : style.accent)
      FilePathText(message.text, workspaceId: workspaceId)
        .textSelection(.enabled)
      if let attachments = message.attachments, !attachments.isEmpty {
        AgentAttachmentChips(
          items: attachments.enumerated().map {
            AgentAttachmentPresentation(attachment: $0.element, index: $0.offset)
          }, style: style)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.horizontal, PaneInteriorMetrics.cardPadding)
    .padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
    .paneInteriorFill(style.canvas)
  }
}

/// A tool call as a plain row: kind, title and paths in mono, status at the trailing edge.
private struct AgentToolRow: View {
  let tool: ToolCall
  let workspaceId: String
  let style: AgentConversationStyle

  var body: some View {
    HStack(alignment: .firstTextBaseline, spacing: PaneInteriorMetrics.rowSpacing) {
      VStack(alignment: .leading, spacing: PaneInteriorMetrics.tightSpacing) {
        HStack(alignment: .firstTextBaseline, spacing: PaneInteriorMetrics.rowSpacing) {
          if let kind = tool.kind, !kind.isEmpty {
            Text(kind.capitalized).font(style.monoFont.weight(.bold)).foregroundStyle(style.ink)
          }
          Text(tool.title).font(style.monoFont).foregroundStyle(style.secondary)
            .lineLimit(2)
        }
        ForEach(Array(tool.locations.enumerated()), id: \.offset) { _, location in
          FilePathText(
            location.line.map { "\(location.path):\(UInt64($0) + 1)" } ?? location.path,
            workspaceId: workspaceId
          )
          .font(style.monoSmallFont).foregroundStyle(style.secondary).textSelection(.enabled)
        }
      }
      Spacer(minLength: PaneInteriorMetrics.compactSpacing)
      Text(statusText).font(style.monoSmallFont).foregroundStyle(statusColor)
    }
    .padding(.vertical, PaneInteriorMetrics.rowVerticalPadding)
    .paneInteriorSeparator(style.border)
    .accessibilityElement(children: .combine)
  }

  private var statusText: String {
    switch tool.status {
    case .pending: "pending"
    case .in_progress: "● running"
    case .completed: "✓ done"
    case .failed: "✕ failed"
    }
  }

  private var statusColor: Color {
    switch tool.status {
    case .pending: style.secondary
    case .in_progress: style.accent
    case .completed: style.success
    case .failed: style.danger
    }
  }
}

/// The permission card: the request in a flat `canvas` card with Allow as the primary pill,
/// other approvals as secondary pills and rejections in `danger` text.
struct AgentPermissionCard: View {
  let client: EngineClient
  let request: PermissionRequest
  let style: AgentConversationStyle

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.rowSpacing) {
      Text(request.title).font(style.headingFont).foregroundStyle(style.ink)
      if !request.detail.isEmpty {
        Text(request.detail).font(style.bodyFont).foregroundStyle(style.secondary)
          .textSelection(.enabled)
      }
      if let presentation = request.presentation {
        PermissionPresentationView(presentation: presentation, style: style)
      }
      HStack(spacing: PaneInteriorMetrics.compactSpacing) {
        ForEach(request.options, id: \.optionId) { option in
          Button(option.name) { client.answerPermission(request, optionId: option.optionId) }
            .buttonStyle(PanePillButtonStyle(Self.role(option.kind), style: style))
        }
      }
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .frame(maxWidth: .infinity, alignment: .leading)
    .paneInteriorFill(style.canvas)
  }

  static func role(_ kind: PermissionOptionKind) -> PanePillButtonStyle.Role {
    switch kind {
    case .allow_once: .primary
    case .allow_always: .secondary
    case .reject_once, .reject_always: .destructive
    }
  }
}

struct AgentElicitationCard: View {
  let client: EngineClient
  let request: ElicitationRequest
  let style: AgentConversationStyle
  @State private var values: [String: String] = [:]

  private var fields: [(String, String)] {
    guard case .object(let schema) = request.schema,
      case .object(let properties) = schema["properties"]
    else { return [] }
    return properties.keys.sorted().map { key in
      (key, properties[key]?["title"].string ?? key)
    }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.rowSpacing) {
      Label(request.title, systemImage: "list.clipboard").font(style.headingFont)
      ForEach(fields, id: \.0) { field in
        TextField(
          field.1,
          text: Binding(
            get: { values[field.0, default: ""] }, set: { values[field.0] = $0 })
        )
        .textFieldStyle(.plain).font(style.bodyFont)
        .padding(.horizontal, PaneInteriorMetrics.rowSpacing)
        .padding(.vertical, PaneInteriorMetrics.rowVerticalPadding)
        .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
      }
      HStack {
        Button("Cancel", role: .cancel) {
          client.answerElicitation(request, fields: nil, cancel: true)
        }
        .buttonStyle(PanePillButtonStyle(.secondary, style: style))
        Spacer()
        Button("Submit") {
          client.answerElicitation(
            request, fields: .object(values.mapValues(JsonValue.string)), cancel: false)
        }
        .buttonStyle(PanePillButtonStyle(.primary, style: style)).disabled(fields.isEmpty)
      }
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .paneInteriorFill(style.canvas)
  }
}

struct AgentWebElementAttachmentCard: View {
  let draft: AgentWebAttachmentDraft
  let style: AgentConversationStyle
  let remove: () -> Void

  private var element: WebPickedElement { draft.package.elements[0] }

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.rowSpacing) {
      HStack(spacing: PaneInteriorMetrics.compactSpacing) {
        Image(systemName: "cursorarrow.click.2").foregroundStyle(style.accent)
        Text("Page element attached to this request").font(style.captionFont.weight(.bold))
        Spacer()
        Button(action: remove) { Image(systemName: "xmark").foregroundStyle(style.muted) }
          .buttonStyle(.plain).accessibilityLabel("Remove selected page elements")
      }
      field("page", draft.package.url)
      field("selector", element.selector)
      if let component = element.component {
        field("source", source(component), color: style.success)
      }
      field("html", element.outerHtml)
      field("styles", styleSummary(element.styles))
      field(
        "accessibility",
        "role \(element.accessibility.role) · name \(element.accessibility.name) · "
          + element.accessibility.state)
      field(
        "position",
        "x \(element.viewportRect.x) · y \(element.viewportRect.y) · viewport "
          + "\(element.viewportRect.width)×\(element.viewportRect.height)")
      imagePreview
      if draft.package.elements.count > 1 {
        Text("\(draft.package.elements.count) elements selected")
          .font(style.captionFont).foregroundStyle(style.secondary)
      }
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .paneInteriorFill(style.canvas)
  }

  private func field(_ label: String, _ value: String, color: Color? = nil) -> some View {
    HStack(alignment: .top, spacing: PaneInteriorMetrics.rowSpacing) {
      Text(label).font(style.captionFont).foregroundStyle(style.muted)
        .frame(width: AgentAttachmentMetrics.labelWidth, alignment: .leading)
      Text(value).font(style.monoSmallFont).foregroundStyle(color ?? style.ink).lineLimit(2)
        .textSelection(.enabled)
    }
  }

  @ViewBuilder private var imagePreview: some View {
    #if os(macOS)
      if let first = draft.images.first, let image = NSImage(data: first.bytes) {
        Image(nsImage: image).resizable().scaledToFit()
          .frame(maxWidth: .infinity, maxHeight: AgentAttachmentMetrics.previewHeight)
          .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
      }
    #else
      EmptyView()
    #endif
  }

  private func source(_ component: WebComponentSource) -> String {
    let location =
      component.sourceFile.map { file in
        " · \(file)\(component.line.map { ":\($0)" } ?? "")"
      } ?? ""
    return "\(component.component) · \(component.framework)\(location)"
  }

  private func styleSummary(_ value: WebComputedStyles) -> String {
    "\(value.color) on \(value.background) · \(value.font) · radius \(value.borderRadius)"
  }
}

private enum AgentAttachmentMetrics {
  static let labelWidth: CGFloat = 72
  static let previewHeight: CGFloat = 96
}
