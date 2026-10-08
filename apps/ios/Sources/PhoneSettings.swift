import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct PhoneSettings: View {
  @Bindable var client: EngineClient
  @Environment(\.dismiss) private var dismiss
  @State private var selectedProvider: UsageProvider?

  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: 24) {
          Text("Usage").font(.title2.bold())
          Text("Subscription windows reported by the provider tools signed in on your Mac.")
            .font(.subheadline)
            .foregroundStyle(PhonePalette.secondary)
          if client.usageReadings.isEmpty {
            ContentUnavailableView(
              "No usage readers", systemImage: "chart.donut",
              description: Text("Connect to the Mac and try again."))
          } else {
            rings
            if let reading = selectedReading { PhoneUsageDetail(reading: reading) }
          }
        }
        .padding(20)
      }
      .background(PhonePalette.canvas)
      .foregroundStyle(PhonePalette.ink)
      .navigationTitle("Settings")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Done") { dismiss() }
        }
        ToolbarItem(placement: .primaryAction) {
          Button(action: refresh) {
            Label("Refresh", systemImage: "arrow.clockwise")
          }
          .accessibilityIdentifier("usage-refresh")
        }
      }
      .onAppear { chooseProvider() }
      .onChange(of: client.usageReadings.map(\.provider)) { _, _ in chooseProvider() }
      .task { await refreshPeriodically() }
    }
    .tint(PhonePalette.accent)
  }

  private var rings: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: 18) {
        ForEach(client.usageReadings, id: \.provider) { reading in
          Button {
            selectedProvider = reading.provider
          } label: {
            PhoneUsageRing(reading: reading, selected: reading.provider == selectedProvider)
          }
          .buttonStyle(.plain)
        }
      }
      .padding(.horizontal, 2)
      .padding(.vertical, 6)
    }
    .accessibilityIdentifier("usage-rings")
  }

  private var selectedReading: UsageReading? {
    client.usageReadings.first { $0.provider == selectedProvider }
  }

  private func chooseProvider() {
    if selectedReading == nil { selectedProvider = client.usageReadings.first?.provider }
  }

  private func refresh() {
    Task { await client.refreshUsageInBackground(selectedProvider) }
  }

  private func refreshPeriodically() async {
    while !Task.isCancelled {
      do { try await Task.sleep(for: .seconds(300)) } catch { return }
      await client.refreshUsageInBackground()
    }
  }
}

private struct PhoneUsageRing: View {
  let reading: UsageReading
  let selected: Bool

  var body: some View {
    VStack(spacing: 5) {
      ZStack {
        Circle()
          .stroke(PhonePalette.secondary.opacity(0.28), lineWidth: 7)
        UsageRingArc(presentation, lineWidth: 5)
        ProviderGlyph(reading.provider)
          .foregroundStyle(PhonePalette.ink)
          .frame(width: 24, height: 24)
      }
      .frame(width: 58, height: 58)
      Text(presentation.percentage)
        .font(.caption.bold())
        .foregroundStyle(PhonePalette.secondary)
    }
    .opacity(presentation.emphasis.ringOpacity)
    .padding(10)
    .background(selected ? PhonePalette.face : Color.clear, in: RoundedRectangle(cornerRadius: 16))
    .overlay {
      if selected {
        RoundedRectangle(cornerRadius: 16).stroke(PhonePalette.accent.opacity(0.45), lineWidth: 1)
      }
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel("\(reading.provider.displayName) usage \(presentation.percentage)")
    .accessibilityIdentifier("usage-ring-\(reading.provider.rawValue)")
  }

  private var presentation: UsagePresentation { UsagePresentation(reading) }
}

private struct PhoneUsageDetail: View {
  let reading: UsageReading

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack(spacing: 10) {
        ProviderGlyph(reading.provider).frame(width: 22, height: 22)
        Text(reading.provider.displayName).font(.headline)
        Spacer()
        Circle().fill(statusColor).frame(width: 9, height: 9)
      }
      if reading.state == .login_needed {
        stateMessage("Sign in in your terminal.", icon: "person.crop.circle.badge.exclamationmark")
      } else if reading.state == .no_access {
        stateMessage("Allow access in Settings on your Mac.", icon: "lock")
      } else if let primary = reading.primary {
        PhoneUsageWindow(window: primary, stale: reading.state == .stale)
        if let secondary = reading.secondary {
          PhoneUsageWindow(window: secondary, stale: reading.state == .stale)
        }
        if let hint = UsagePresentation(reading).staleHint {
          Text(hint).font(.caption2).foregroundStyle(PhonePalette.secondary)
        }
      } else {
        stateMessage(stateLabel, icon: "exclamationmark.circle")
      }
      Text("Source: \(reading.source.displayName) · \(UsageTimeText.age(of: reading))")
        .font(.caption2)
        .foregroundStyle(PhonePalette.secondary)
    }
    .padding(18)
    .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 20))
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("usage-detail-\(reading.provider.rawValue)")
  }

  private func stateMessage(_ message: String, icon: String) -> some View {
    Label(message, systemImage: icon)
      .font(.subheadline.weight(.semibold))
      .foregroundStyle(PhonePalette.secondary)
      .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
  }

  private var stateLabel: String {
    switch reading.state {
    case .ok: "No reading yet."
    case .stale: "Last reading is stale."
    case .login_needed: "Sign in in your terminal."
    case .no_access: "Allow access in Settings on your Mac."
    case .unavailable: UsagePresentation(reading).unavailableMessage
    }
  }

  private var statusColor: Color {
    switch reading.state {
    case .ok: PhonePalette.success
    case .stale: PhonePalette.warning
    case .login_needed, .no_access, .unavailable: PhonePalette.secondary
    }
  }
}

private struct PhoneUsageWindow: View {
  let window: UsageWindow
  let stale: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      HStack {
        Text(window.label).font(.subheadline.weight(.semibold))
        Spacer()
        Text("\(Int((fraction * 100).rounded()))%").font(.caption.bold())
      }
      ProgressView(value: fraction)
        .tint(UsageBand(usedFraction: fraction).color)
        .opacity(stale ? UsageEmphasis.lastKnown.arcOpacity : 1)
      Text(UsageTimeText.reset(window.resetsAt))
        .font(.caption2)
        .foregroundStyle(PhonePalette.secondary)
    }
  }

  private var fraction: Double { min(max(window.used, 0), 1) }
}
