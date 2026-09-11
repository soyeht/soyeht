import Foundation
import SoyehtProtocol
import SwiftUI

private enum AgentColors {
  static let canvas = Color(red: 224 / 255, green: 229 / 255, blue: 236 / 255)
  static let face = Color(red: 232 / 255, green: 237 / 255, blue: 244 / 255)
  static let ink = Color(red: 62 / 255, green: 74 / 255, blue: 102 / 255)
  static let secondary = Color(red: 110 / 255, green: 122 / 255, blue: 150 / 255)
  static let accent = Color(red: 91 / 255, green: 124 / 255, blue: 250 / 255)
  static let shadow = Color(red: 166 / 255, green: 180 / 255, blue: 200 / 255)
}

private struct AgentRaised: ViewModifier {
  var radius: CGFloat = 14
  func body(content: Content) -> some View {
    content.background(AgentColors.face, in: RoundedRectangle(cornerRadius: radius))
      .shadow(color: .white.opacity(0.78), radius: 5, x: -3, y: -3)
      .shadow(color: AgentColors.shadow.opacity(0.42), radius: 5, x: 3, y: 3)
  }
}

@MainActor public struct AgentConversationView: View {
  @Bindable private var client: EngineClient
  private let paneId: String
  private let showsConfiguration: Bool
  @State private var prompt = ""
  @FocusState private var promptFocused: Bool

  public init(client: EngineClient, paneId: String, showsConfiguration: Bool = true) {
    self.client = client
    self.paneId = paneId
    self.showsConfiguration = showsConfiguration
  }

  private var pane: Pane? { client.panes.first { $0.paneId == paneId } }
  private var view: AgentPaneView { client.agentPanes[paneId] ?? AgentPaneView() }
  private var isWorking: Bool {
    guard let state = pane?.state else { return false }
    return [.working, .waiting_for_you, .cancelling, .no_signal].contains(state)
  }

  public var body: some View {
    VStack(spacing: 0) {
      controls
      Divider().opacity(0.35)
      transcript
      if !view.authMethods.isEmpty { authentication }
      if let permission = view.permission { PermissionCard(client: client, request: permission) }
      if let elicitation = view.elicitation {
        ElicitationCard(client: client, request: elicitation).id(elicitation.requestId)
      }
      if !view.queue.isEmpty || view.queuePaused { queue }
      composer
      if showsConfiguration, let options = pane?.agent?.options, !options.isEmpty {
        configuration(options)
      }
    }
    .background(AgentColors.canvas).foregroundStyle(AgentColors.ink)
    .task(id: prompt.isEmpty) {
      guard !prompt.isEmpty else { return }
      while !Task.isCancelled && !prompt.isEmpty {
        client.setDraft(paneId, active: true)
        do { try await Task.sleep(for: .seconds(8)) } catch { return }
      }
    }
    .onDisappear {
      if !prompt.isEmpty { client.setDraft(paneId, active: false) }
    }
  }

  private var controls: some View {
    HStack(spacing: 10) {
      if let usage = contextUsage { ContextRing(used: usage.0, total: usage.1) }
      if let agent = pane?.agent {
        Button {
          client.setYolo(paneId, on: !agent.yolo)
        } label: {
          Label(agent.yolo ? "Yolo on" : "Yolo off", systemImage: agent.yolo ? "bolt.fill" : "bolt")
        }
        .buttonStyle(.bordered).tint(agent.yolo ? .orange : AgentColors.secondary)
        .disabled(agent.operation != nil || !client.connected)
      }
      Spacer()
      if isWorking {
        Button(role: .destructive) {
          client.cancelAgent(paneId)
        } label: {
          Label("Cancel", systemImage: "stop.fill")
        }.buttonStyle(.bordered).disabled(pane?.state == .cancelling)
      }
    }.font(.caption).padding(.horizontal, 16).padding(.vertical, 10)
  }

  private var transcript: some View {
    ScrollViewReader { proxy in
      ScrollView {
        LazyVStack(spacing: 12) {
          if view.hasOlderMessages {
            Button("Load earlier messages") { client.loadOlderAgentMessages(paneId) }
              .font(.caption).buttonStyle(.bordered)
          }
          if let notice = view.notice {
            Label(notice, systemImage: "exclamationmark.triangle")
              .font(.caption).foregroundStyle(.orange).padding(10)
          }
          ForEach(view.messages, id: \.localId) { message in
            TranscriptRow(message: message).id(message.localId)
          }
          Color.clear.frame(height: 1).id("agent-bottom")
        }.padding(16)
      }
      .onChange(of: view.messages.count) { _, _ in
        withAnimation(.easeOut(duration: 0.18)) { proxy.scrollTo("agent-bottom", anchor: .bottom) }
      }
    }
  }

  private var queue: some View {
    VStack(alignment: .leading, spacing: 8) {
      HStack {
        Label(
          view.queuePaused ? "Queue paused after interruption" : "\(view.queue.count) queued",
          systemImage: view.queuePaused
            ? "pause.circle" : "text.line.last.and.arrowtriangle.forward")
        Spacer()
        if view.queuePaused {
          Button("Resume") { client.resumeAgentQueue(paneId) }.buttonStyle(.borderedProminent)
        }
      }.font(.caption.weight(.semibold))
      ForEach(view.queue, id: \.promptId) { item in
        HStack(spacing: 8) {
          Text(item.text).font(.caption).lineLimit(2)
          Spacer()
          unavailableSteer
          Button(role: .destructive) {
            client.removeQueuedPrompt(paneId, promptId: item.promptId)
          } label: {
            Image(systemName: "xmark")
          }
          .buttonStyle(.plain).accessibilityLabel("Remove queued message")
        }
      }
    }.padding(12).modifier(AgentRaised(radius: 12)).padding(.horizontal, 16).padding(.bottom, 10)
  }

  private var authentication: some View {
    VStack(alignment: .leading, spacing: 10) {
      Label("Sign in to continue", systemImage: "person.badge.key")
        .font(.headline).foregroundStyle(.orange)
      #if os(macOS)
        ForEach(view.authMethods, id: \.id) { method in
          Button {
            client.startAgentAuthentication(paneId, methodId: method.id)
          } label: {
            HStack {
              VStack(alignment: .leading, spacing: 3) {
                Text(method.name).fontWeight(.semibold)
                if let detail = method.description {
                  Text(detail).font(.caption).foregroundStyle(AgentColors.secondary)
                }
              }
              Spacer()
              Image(systemName: method.terminal ? "terminal" : "arrow.right.circle")
            }
          }.buttonStyle(.bordered)
        }
      #else
        Text("Finish sign-in on the Mac, then return here.")
          .font(.caption).foregroundStyle(AgentColors.secondary)
      #endif
    }.padding(14).modifier(AgentRaised(radius: 14)).padding(.horizontal, 16).padding(.bottom, 10)
  }

  @ViewBuilder private var unavailableSteer: some View {
    let button = Button("Send now") {}.buttonStyle(.borderless).disabled(true)
      .accessibilityHint("This agent only reads new messages between turns")
    #if os(macOS)
      button.help("This agent only reads new messages between turns")
    #else
      button
    #endif
  }

  private var composer: some View {
    HStack(alignment: .bottom, spacing: 10) {
      TextField("Message the agent", text: $prompt, axis: .vertical)
        .lineLimit(1...6).textFieldStyle(.plain).focused($promptFocused)
        .onSubmit { submit() }
        .padding(.horizontal, 14).padding(.vertical, 11).modifier(AgentRaised(radius: 14))
      Button(action: submit) {
        Image(systemName: "arrow.up").font(.headline).foregroundStyle(.white)
          .frame(width: 36, height: 36).background(AgentColors.accent, in: Circle())
      }.buttonStyle(.plain).disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        .accessibilityLabel("Send message")
    }.padding(.horizontal, 16).padding(.vertical, 12)
  }

  private func configuration(_ options: [ConfigOption]) -> some View {
    HStack(spacing: 8) {
      ForEach(options, id: \.id) { option in
        if option.type == .select {
          Menu {
            ForEach(option.options, id: \.value) { choice in
              Button(choice.name) {
                client.setAgentConfig(paneId, configId: option.id, value: .v0(choice.value))
              }
            }
          } label: {
            Label(configValue(option.currentValue), systemImage: configIcon(option))
          }
          .disabled(pane?.agent?.operation != nil)
        } else if case .v1(let enabled) = option.currentValue {
          Button {
            client.setAgentConfig(paneId, configId: option.id, value: .v1(!enabled))
          } label: {
            Label(option.name, systemImage: enabled ? "bolt.fill" : "bolt")
          }
          .disabled(pane?.agent?.operation != nil)
        }
      }
      Spacer()
      if pane?.agent?.operation != nil { ProgressView().controlSize(.small) }
    }.font(.caption).padding(.horizontal, 16).padding(.bottom, 12)
  }

  private var contextUsage: (Double, Double)? {
    guard let usage = view.messages.reversed().compactMap(\.usage).first,
      let used = Double(usage.totalTokens), let totalText = usage.contextSize,
      let total = Double(totalText), total > 0
    else { return nil }
    return (min(max(used, 0), total), total)
  }

  private func submit() {
    let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    prompt = ""
    client.setDraft(paneId, active: false)
    client.sendPrompt(paneId, text: text)
    promptFocused = true
  }

  private func configValue(_ value: ConfigValue) -> String {
    switch value {
    case .v0(let text): text
    case .v1(let enabled): enabled ? "On" : "Off"
    }
  }

  private func configIcon(_ option: ConfigOption) -> String {
    switch option.category ?? option.id {
    case "model": "cpu"
    case "thought_level": "brain"
    case "mode": "shield.checkered"
    default: "slider.horizontal.3"
    }
  }
}

private struct ContextRing: View {
  let used: Double
  let total: Double
  private var fraction: Double { used / total }
  private var color: Color {
    fraction > 0.9 ? .red : fraction > 0.75 ? .orange : AgentColors.accent
  }
  var body: some View {
    Menu {
      Text("\(Int(used)) of \(Int(total)) context tokens")
    } label: {
      ZStack {
        Circle().stroke(AgentColors.shadow.opacity(0.35), lineWidth: 3)
        Circle().trim(from: 0, to: fraction).stroke(
          color, style: StrokeStyle(lineWidth: 3, lineCap: .round)
        )
        .rotationEffect(.degrees(-90))
      }.frame(width: 22, height: 22)
    }.accessibilityLabel("Context use \(Int(fraction * 100)) percent")
  }
}

private struct TranscriptRow: View {
  let message: TranscriptMessage
  @State private var showsThought = false
  var body: some View {
    Group {
      switch message.kind {
      case .thought:
        DisclosureGroup("Thought", isExpanded: $showsThought) {
          Text(message.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, 6)
        }.foregroundStyle(AgentColors.secondary)
      case .tool:
        if let tool = message.tool {
          HStack(alignment: .top, spacing: 10) {
            Image(
              systemName: tool.status == .failed
                ? "exclamationmark.triangle" : "wrench.and.screwdriver")
            VStack(alignment: .leading, spacing: 4) {
              Text(tool.title).fontWeight(.semibold)
              Text(tool.status.rawValue.replacingOccurrences(of: "_", with: " ")).font(.caption)
              ForEach(Array(tool.locations.enumerated()), id: \.offset) { _, location in
                Text(location.line.map { "\(location.path):\($0)" } ?? location.path)
                  .font(.caption.monospaced()).textSelection(.enabled)
              }
            }
            Spacer()
          }.padding(11).modifier(AgentRaised(radius: 11))
        }
      case .terminal:
        if let output = message.terminal {
          Text(Data(base64Encoded: output.bytes).map { String(decoding: $0, as: UTF8.self) } ?? "")
            .font(.caption.monospaced()).textSelection(.enabled).frame(
              maxWidth: .infinity, alignment: .leading
            )
            .padding(11).background(
              Color.black.opacity(0.06), in: RoundedRectangle(cornerRadius: 10))
        }
      case .plan:
        VStack(alignment: .leading, spacing: 5) {
          Label("Plan", systemImage: "list.bullet.clipboard").fontWeight(.semibold)
          ForEach(Array((message.plan ?? []).enumerated()), id: \.offset) { _, entry in
            Label(
              entry.content,
              systemImage: entry.status == .completed ? "checkmark.circle.fill" : "circle")
          }
        }.font(.caption).frame(maxWidth: .infinity, alignment: .leading)
      case .usage:
        EmptyView()
      default:
        Text(message.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
          .padding(12)
          .background(
            message.role == .user ? AgentColors.accent.opacity(0.15) : AgentColors.face,
            in: RoundedRectangle(cornerRadius: 14)
          )
          .padding(.leading, message.role == .user ? 48 : 0)
          .padding(.trailing, message.role == .user ? 0 : 48)
      }
    }.font(.body)
  }
}

private struct PermissionCard: View {
  let client: EngineClient
  let request: PermissionRequest
  var body: some View {
    VStack(alignment: .leading, spacing: 9) {
      Label(request.title, systemImage: "shield.lefthalf.filled.badge.checkmark")
        .font(.headline).foregroundStyle(.orange)
      Text(request.detail).font(.caption).textSelection(.enabled)
      HStack {
        ForEach(request.options, id: \.optionId) { option in
          if option.kind == .allow_once {
            Button(option.name) { client.answerPermission(request, optionId: option.optionId) }
              .buttonStyle(.borderedProminent)
          } else {
            Button(option.name) { client.answerPermission(request, optionId: option.optionId) }
              .buttonStyle(.bordered)
          }
        }
      }
    }.padding(14).modifier(AgentRaised(radius: 14)).padding(.horizontal, 16).padding(.bottom, 10)
  }
}

private struct ElicitationCard: View {
  let client: EngineClient
  let request: ElicitationRequest
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
    VStack(alignment: .leading, spacing: 9) {
      Label(request.title, systemImage: "list.clipboard").font(.headline)
      ForEach(fields, id: \.0) { field in
        TextField(
          field.1,
          text: Binding(
            get: { values[field.0, default: ""] }, set: { values[field.0] = $0 })
        )
        .textFieldStyle(.roundedBorder)
      }
      HStack {
        Button("Cancel", role: .cancel) {
          client.answerElicitation(request, fields: nil, cancel: true)
        }
        Spacer()
        Button("Submit") {
          client.answerElicitation(
            request, fields: .object(values.mapValues(JsonValue.string)), cancel: false)
        }.buttonStyle(.borderedProminent).disabled(fields.isEmpty)
      }
    }.padding(14).modifier(AgentRaised(radius: 14)).padding(.horizontal, 16).padding(.bottom, 10)
  }
}
