import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct PhoneSettings: View {
  @Bindable var client: EngineClient
  @Environment(\.dismiss) private var dismiss
  @State private var selectedProvider: UsageProvider?
  @State private var scrolledTo: String?

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
          dictation.id(Self.dictationSection)
        }
        .scrollTargetLayout()
        .padding(20)
      }
      .scrollPosition(id: $scrolledTo)
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
      .onAppear {
        chooseProvider()
        // The shortcut sent the person here to install an engine.
        if DictationEngineStore.shared.installPrompt != nil { scrolledTo = Self.dictationSection }
      }
      .onChange(of: client.usageReadings.map(\.provider)) { _, _ in chooseProvider() }
      .task { await refreshPeriodically() }
    }
    .tint(PhonePalette.accent)
  }

  private static let dictationSection = "dictation"

  private var dictation: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("Dictation").font(.title2.bold())
      Text(DictationPreferencesView.summary)
        .font(.subheadline)
        .foregroundStyle(PhonePalette.secondary)
      DictationPreferencesView(store: .shared, style: .standard)
    }
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
        UsageRingGauge(
          presentation, geometry: UsageRingGeometry(arcWidth: 5, trackWidth: 7),
          track: PhonePalette.secondary.opacity(0.28))
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

  private var model: UsageDetailModel { UsageDetailModel(reading, onThisMac: false) }

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack(spacing: 10) {
        ProviderGlyph(reading.provider).frame(width: 22, height: 22)
        Text(model.providerName).font(.headline)
        Spacer()
        Circle().fill(model.statusColor(in: SoyehtThemeColors.active)).frame(width: 9, height: 9)
      }
      switch model.content {
      case .notice(let notice):
        stateMessage(notice)
      case .windows(let rows, let hint):
        ForEach(Array(rows.enumerated()), id: \.offset) { PhoneUsageWindow(row: $0.element) }
        if let hint {
          Text(hint).font(.caption2).foregroundStyle(PhonePalette.secondary)
        }
      }
      Text(model.footer)
        .font(.caption2)
        .foregroundStyle(PhonePalette.secondary)
    }
    .padding(18)
    .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 20))
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("usage-detail-\(reading.provider.rawValue)")
  }

  private func stateMessage(_ notice: UsageDetailModel.Notice) -> some View {
    Label(notice.message, systemImage: notice.symbol)
      .font(.subheadline.weight(.semibold))
      .foregroundStyle(PhonePalette.secondary)
      .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
  }
}

private struct PhoneUsageWindow: View {
  let row: UsageDetailModel.Row

  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      HStack {
        Text(row.label).font(.subheadline.weight(.semibold))
        Spacer()
        Text(row.percentage).font(.caption.bold())
      }
      ProgressView(value: row.fraction)
        .tint(row.band.color)
        .opacity(row.barOpacity)
      Text(row.reset)
        .font(.caption2)
        .foregroundStyle(PhonePalette.secondary)
    }
  }
}
