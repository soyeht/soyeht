import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct UsageRing: View {
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
            .fill(MacTheme.face)
            .overlay {
              Circle().stroke(MacTheme.well, lineWidth: DesignTokens.UsageRing.trackWidth)
            }
            .frame(
              width: DesignTokens.UsageRing.faceDiameter,
              height: DesignTokens.UsageRing.faceDiameter)
          UsageRingArc(presentation, lineWidth: DesignTokens.UsageRing.arcWidth)
            .frame(width: DesignTokens.UsageRing.diameter, height: DesignTokens.UsageRing.diameter)
          ProviderGlyph(reading.provider)
            .foregroundStyle(MacTheme.ink)
            .frame(
              width: DesignTokens.UsageRing.glyphSize, height: DesignTokens.UsageRing.glyphSize)
        }
        .frame(width: DesignTokens.UsageRing.diameter, height: DesignTokens.UsageRing.diameter)
        Text(presentation.percentage)
          .font(MacTheme.chrome(DesignTokens.UsageRing.percentageSize, weight: .bold))
          .foregroundStyle(presentation.isStale ? MacTheme.muted : MacTheme.secondary)
          .frame(height: DesignTokens.UsageRing.percentageHeight)
      }
      .opacity(presentation.emphasis.ringOpacity)
      .frame(width: DesignTokens.UsageRing.diameter, height: DesignTokens.UsageRing.height)
    }
    .buttonStyle(.plain)
    .help("\(reading.provider.displayName) usage")
    .accessibilityLabel("\(reading.provider.displayName) usage \(presentation.percentage)")
    .accessibilityValue(presentation.isStale ? "Last known reading" : "")
    .accessibilityIdentifier("usage-ring-\(reading.provider.rawValue)")
    .onHover { value in
      withAnimation(.easeOut(duration: 0.12)) { hovering = value }
    }
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
}

struct UsageDetailBalloon: View {
  let reading: UsageReading

  private var model: UsageDetailModel { UsageDetailModel(reading, onThisMac: true) }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
      header
      switch model.content {
      case .notice(let notice):
        stateMessage(notice)
      case .windows(let rows, let hint):
        ForEach(Array(rows.enumerated()), id: \.offset) { UsageWindowRow(row: $0.element) }
        if let hint {
          Text(hint)
            .font(MacTheme.chrome(MacTheme.TypeScale.micro))
            .foregroundStyle(MacTheme.muted)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      Text(model.footer)
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
      Circle().fill(model.statusColor(in: SoyehtThemeColors.active)).frame(width: 8, height: 8)
      Text(model.providerName)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      Spacer(minLength: MacTheme.Space.x4)
      Text(model.reset)
        .font(MacTheme.chrome(MacTheme.TypeScale.micro))
        .foregroundStyle(MacTheme.muted)
    }
    .frame(height: 16)
  }

  private func stateMessage(_ notice: UsageDetailModel.Notice) -> some View {
    Label(notice.message, systemImage: notice.symbol)
      .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
      .foregroundStyle(MacTheme.secondary)
      .fixedSize(horizontal: false, vertical: true)
      .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
  }
}

private struct UsageWindowRow: View {
  let row: UsageDetailModel.Row

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
      HStack(spacing: MacTheme.Space.x4) {
        Text(row.label)
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        Spacer(minLength: MacTheme.Space.x4)
        Text(row.percentage)
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
        Text(row.reset)
          .font(MacTheme.chrome(MacTheme.TypeScale.micro))
          .foregroundStyle(MacTheme.muted)
      }
      GeometryReader { geometry in
        ZStack(alignment: .leading) {
          Capsule().fill(MacTheme.well)
          Capsule().fill(row.band.color)
            .opacity(row.barOpacity)
            .frame(width: geometry.size.width * row.fraction)
        }
      }
      .frame(height: 6)
    }
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
      ViewThatFits(in: .horizontal) {
        HStack(alignment: .top, spacing: MacTheme.Space.x16) {
          providerList(width: 210)
          selectedDetail
          Spacer(minLength: 0)
        }
        ScrollView(.vertical) {
          VStack(spacing: MacTheme.Space.x12) {
            providerList(width: nil)
            selectedDetail
          }
          .frame(maxWidth: .infinity)
        }
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

  private func providerList(width: CGFloat?) -> some View {
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
          .foregroundStyle(reading.state == .stale ? MacTheme.muted : MacTheme.ink)
          .padding(.horizontal, MacTheme.Space.x12)
          .frame(maxWidth: .infinity, minHeight: 42)
          .background(
            selectedProvider == reading.provider ? MacTheme.well : Color.clear,
            in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
        }
        .buttonStyle(.plain)
      }
    }
    .padding(MacTheme.Space.x8)
    .frame(width: width)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
  }

  @ViewBuilder private var selectedDetail: some View {
    if let reading = selectedReading {
      UsageDetailBalloon(reading: reading)
    } else {
      Text("No usage reader is available.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
        .frame(maxWidth: .infinity, minHeight: 122)
    }
  }

  private func chooseFirstProvider() {
    if selectedReading == nil { selectedProvider = client.usageReadings.first?.provider }
  }

  private func refresh() {
    Task { await client.refreshUsageInBackground(selectedProvider) }
  }
}
