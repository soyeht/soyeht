import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct UsageRing: View {
  @Bindable var client: EngineClient
  let reading: UsageReading
  let onDetails: (UsageProvider) -> Void
  @State private var hovering = false

  var body: some View {
    Button {
      hovering = false
      onDetails(reading.provider)
    } label: {
      VStack(spacing: 0) {
        ZStack {
          Circle()
            .stroke(MacTheme.shadowDark.opacity(0.42), lineWidth: 5)
            .frame(width: 34, height: 34)
          if let fraction = presentation.fraction {
            Circle()
              .trim(from: 0, to: fraction)
              .stroke(
                bandColor, style: StrokeStyle(lineWidth: 3, lineCap: .round)
              )
              .rotationEffect(.degrees(-90))
              .frame(width: 40, height: 40)
          }
          ProviderGlyph(reading.provider)
            .foregroundStyle(MacTheme.ink)
            .frame(width: 16, height: 16)
        }
        .frame(width: 40, height: 40)
        Text(presentation.percentage)
          .font(MacTheme.chrome(9, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
          .frame(height: 15)
      }
      .opacity(presentation.fraction == nil ? 0.46 : 1)
      .frame(width: 40, height: 55)
    }
    .buttonStyle(.plain)
    .help("\(reading.provider.displayName) usage")
    .accessibilityLabel("\(reading.provider.displayName) usage \(presentation.percentage)")
    .accessibilityIdentifier("usage-ring-\(reading.provider.rawValue)")
    .onHover(perform: hoverChanged)
    .overlay(alignment: .topLeading) {
      if hovering {
        UsageDetailBalloon(reading: reading)
          .offset(x: 48, y: -8)
          .allowsHitTesting(false)
          .transition(.opacity.combined(with: .scale(scale: 0.98, anchor: .topLeading)))
      }
    }
    .zIndex(hovering ? 20 : 0)
  }

  private var presentation: UsagePresentation { UsagePresentation(reading) }

  private var bandColor: Color {
    switch presentation.band {
    case .success: MacTheme.success
    case .warning: MacTheme.warning
    case .danger: MacTheme.danger
    case nil: MacTheme.muted
    }
  }

  private func hoverChanged(_ value: Bool) {
    withAnimation(.easeOut(duration: 0.12)) { hovering = value }
    guard value, presentation.needsHoverRefresh() else { return }
    Task { try? await client.refreshUsage(reading.provider) }
  }
}

struct UsageDetailBalloon: View {
  let reading: UsageReading

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
      header
      if reading.state == .login_needed {
        stateMessage("Sign in in your terminal.", icon: "person.crop.circle.badge.exclamationmark")
      } else if reading.state == .no_access {
        stateMessage("Allow access in Settings.", icon: "lock")
      } else if let primary = reading.primary {
        UsageWindowRow(window: primary)
        if let secondary = reading.secondary { UsageWindowRow(window: secondary) }
      } else {
        stateMessage(stateLabel, icon: "exclamationmark.circle")
      }
      Text("Source: \(reading.source.displayName) · \(UsageTimeText.fetched(reading.fetchedAt))")
        .font(MacTheme.chrome(MacTheme.TypeScale.micro))
        .foregroundStyle(MacTheme.muted)
        .lineLimit(1)
    }
    .padding(MacTheme.Space.x12)
    .frame(width: 250, alignment: .leading)
    .foregroundStyle(MacTheme.ink)
    .neoRaised(radius: 14, elevation: .balloon)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("usage-detail-\(reading.provider.rawValue)")
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Circle().fill(statusColor).frame(width: 8, height: 8)
      Text(reading.provider.displayName)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      Spacer(minLength: MacTheme.Space.x4)
      Text(UsageTimeText.reset(reading.primary?.resetsAt))
        .font(MacTheme.chrome(MacTheme.TypeScale.micro))
        .foregroundStyle(MacTheme.muted)
    }
    .frame(height: 16)
  }

  private func stateMessage(_ message: String, icon: String) -> some View {
    Label(message, systemImage: icon)
      .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
      .foregroundStyle(MacTheme.secondary)
      .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
  }

  private var stateLabel: String {
    switch reading.state {
    case .ok: "No reading yet."
    case .stale: "Last reading is stale."
    case .login_needed: "Sign in in your terminal."
    case .no_access: "Allow access in Settings."
    case .unavailable: "Usage is unavailable."
    }
  }

  private var statusColor: Color {
    switch reading.state {
    case .ok: MacTheme.success
    case .stale: MacTheme.warning
    case .login_needed, .no_access, .unavailable: MacTheme.muted
    }
  }
}

private struct UsageWindowRow: View {
  let window: UsageWindow

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
      HStack(spacing: MacTheme.Space.x4) {
        Text(window.label)
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        Spacer(minLength: MacTheme.Space.x4)
        Text("\(Int((min(max(window.used, 0), 1) * 100).rounded()))%")
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
        Text(UsageTimeText.reset(window.resetsAt))
          .font(MacTheme.chrome(MacTheme.TypeScale.micro))
          .foregroundStyle(MacTheme.muted)
      }
      GeometryReader { geometry in
        ZStack(alignment: .leading) {
          Capsule().fill(MacTheme.well)
          Capsule().fill(color).frame(width: geometry.size.width * fraction)
        }
      }
      .frame(height: 6)
    }
  }

  private var fraction: Double { min(max(window.used, 0), 1) }
  private var color: Color {
    if fraction < 0.5 { return MacTheme.success }
    if fraction <= 0.8 { return MacTheme.warning }
    return MacTheme.danger
  }
}

struct UsageSettingsDetail: View {
  @Bindable var client: EngineClient
  @Binding var selectedProvider: UsageProvider?

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      HStack {
        Text("Usage").font(MacTheme.chrome(18, weight: .bold))
        Spacer()
        Button(action: refresh) {
          Label("Refresh", systemImage: "arrow.clockwise")
        }
        .buttonStyle(NeoPillButtonStyle())
      }
      Text("Subscription windows reported by the provider tools signed in on this Mac.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
      HStack(alignment: .top, spacing: MacTheme.Space.x16) {
        VStack(spacing: MacTheme.Space.x8) {
          ForEach(client.usageReadings, id: \.provider) { reading in
            Button {
              selectedProvider = reading.provider
            } label: {
              HStack(spacing: MacTheme.Space.x10) {
                ProviderGlyph(reading.provider).frame(width: 18, height: 18)
                Text(reading.provider.displayName)
                  .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
                Spacer()
                Text(UsagePresentation(reading).percentage)
                  .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
              }
              .foregroundStyle(MacTheme.ink)
              .padding(.horizontal, MacTheme.Space.x12)
              .frame(width: 210, height: 42)
              .background(
                selectedProvider == reading.provider ? MacTheme.well : Color.clear,
                in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
            }
            .buttonStyle(.plain)
          }
        }
        .padding(MacTheme.Space.x8)
        .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
        if let reading = selectedReading {
          UsageDetailBalloon(reading: reading)
        } else {
          Text("No usage reader is available.")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
            .foregroundStyle(MacTheme.secondary)
            .frame(maxWidth: .infinity, minHeight: 122)
        }
        Spacer(minLength: 0)
      }
      Spacer(minLength: 0)
    }
    .padding(.vertical, MacTheme.Space.x16)
    .padding(.horizontal, 22)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    .onAppear { chooseFirstProvider() }
    .onChange(of: client.usageReadings.map(\.provider)) { _, _ in chooseFirstProvider() }
  }

  private var selectedReading: UsageReading? {
    client.usageReadings.first { $0.provider == selectedProvider }
  }

  private func chooseFirstProvider() {
    if selectedReading == nil { selectedProvider = client.usageReadings.first?.provider }
  }

  private func refresh() {
    Task { try? await client.refreshUsage(selectedProvider) }
  }
}

private enum UsageTimeText {
  static func reset(_ value: String?) -> String {
    guard let value, let date = timestamp(value) else { return "Reset unavailable" }
    let seconds = date.timeIntervalSinceNow
    guard seconds > 0 else { return "Reset due" }
    return "Resets in \(duration(seconds))"
  }

  static func fetched(_ value: String) -> String {
    guard let date = timestamp(value) else { return "time unavailable" }
    let seconds = max(0, Date().timeIntervalSince(date))
    if seconds < 60 { return "updated just now" }
    return "updated \(duration(seconds)) ago"
  }

  private static func timestamp(_ value: String) -> Date? {
    ISO8601DateFormatter().date(from: value)
  }

  private static func duration(_ seconds: TimeInterval) -> String {
    let minutes = max(1, Int(seconds) / 60)
    if minutes < 60 { return "\(minutes)m" }
    let hours = minutes / 60
    let remainder = minutes % 60
    if hours < 24 { return remainder == 0 ? "\(hours)h" : "\(hours)h \(remainder)m" }
    let days = hours / 24
    return "\(days)d \(hours % 24)h"
  }
}
