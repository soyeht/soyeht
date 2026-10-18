import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct AppRail: View {
  @Bindable var client: EngineClient
  let destination: WorkspaceDestination
  let onSelect: (WorkspaceDestination) -> Void
  let onSettings: () -> Void
  let onUsageDetails: (UsageProvider) -> Void

  var body: some View {
    VStack(spacing: MacTheme.Space.x6) {
      ScrollView(.vertical) {
        VStack(spacing: MacTheme.Space.x6) {
          ForEach(items, id: \.destination) { item in
            AppRailButton(
              glyph: item.glyph,
              label: item.label,
              selected: destination == item.destination,
              badgeCount: item.destination == .agents ? client.agentAttention.totalWaitingCount : 0,
              isWorking: item.destination == .agents && client.agentAttention.hasWorkingAgent,
              action: { onSelect(item.destination) })
          }
          Rectangle()
            .fill(MacTheme.shadowDark)
            .frame(width: DesignTokens.Layout.railDividerWidth, height: 1)
            .padding(.vertical, MacTheme.Space.x2)
          ForEach(client.usageReadings, id: \.provider) { reading in
            UsageRing(reading: reading, onDetails: onUsageDetails)
          }
        }
      }
      // The scroller would sit over the rings: the column is as wide as a ring.
      .scrollIndicators(.hidden)
      AppRailButton(
        glyph: .system("gearshape"), label: "Settings", selected: false, action: onSettings)
    }
    .padding(.vertical, MacTheme.Space.x12)
    .padding(.horizontal, MacTheme.Space.x8)
    .frame(width: MacPaneLayoutMetrics.railWidth)
    .frame(maxHeight: .infinity)
    .accessibilityIdentifier("app-bar")
  }

  private var items: [AppRailItem] {
    [
      AppRailItem(destination: .agents, glyph: .system("cpu"), label: "Agents"),
      AppRailItem(destination: .editor, glyph: .system("doc.text"), label: "Editor"),
      AppRailItem(destination: .web, glyph: .system("globe"), label: "Web"),
      AppRailItem(destination: .tasks, glyph: .system("checklist"), label: "Tasks"),
      AppRailItem(destination: .terminal, glyph: .system("terminal"), label: "Terminal"),
      AppRailItem(destination: .phone, glyph: .system("iphone"), label: "iPhone"),
      AppRailItem(destination: .texts, glyph: .writing, label: "Texts"),
      AppRailItem(
        destination: .presentations, glyph: .system("rectangle.on.rectangle"),
        label: "Presentations"),
      AppRailItem(
        destination: .screenplays, glyph: .system("movieclapper"), label: "Screenplays"),
    ]
  }
}

private struct AppRailItem {
  let destination: WorkspaceDestination
  let glyph: AppRailGlyph
  let label: String
}

private enum AppRailGlyph {
  case system(String)
  case writing
}

private struct AppRailButton: View {
  let glyph: AppRailGlyph
  let label: String
  let selected: Bool
  var badgeCount = 0
  var isWorking = false
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      railGlyph
        .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
        .frame(
          width: MacPaneLayoutMetrics.railButtonSize, height: MacPaneLayoutMetrics.railButtonSize
        )
        .overlay(alignment: .topTrailing) {
          if badgeCount > 0 {
            AgentAttentionBadge(count: badgeCount)
          } else if isWorking {
            AgentWorkingDot().padding(MacTheme.Space.x2)
          }
        }
        .background {
          if selected {
            RoundedRectangle(cornerRadius: MacTheme.Radius.compact, style: .continuous)
              .fill(MacTheme.well)
          }
        }
        .contentShape(Rectangle())
    }
    .help(label)
    .accessibilityLabel(
      badgeCount > 0
        ? "\(label), \(AgentAttentionPolicy.waitingLabel(count: badgeCount))"
        : isWorking ? "\(label), agents working" : label
    )
    .accessibilityIdentifier("app-bar-\(label.lowercased())")
  }

  @ViewBuilder private var railGlyph: some View {
    switch glyph {
    case .system(let name):
      Image(systemName: name)
        .font(.system(size: DesignTokens.Glyph.rail, weight: .medium))
    case .writing:
      WritingQuillGlyph()
        .stroke(style: StrokeStyle(lineWidth: 1.6, lineCap: .round, lineJoin: .round))
        .frame(width: 19, height: 19)
    }
  }
}

private struct WritingQuillGlyph: Shape {
  func path(in rect: CGRect) -> Path {
    let point: (CGFloat, CGFloat) -> CGPoint = {
      CGPoint(x: rect.minX + rect.width * $0 / 20, y: rect.minY + rect.height * $1 / 20)
    }
    var path = Path()
    path.move(to: point(3, 17))
    path.addCurve(to: point(17.5, 2.5), control1: point(6, 10), control2: point(12, 3))
    path.addCurve(to: point(10, 15), control1: point(18, 8), control2: point(14, 13))
    path.addCurve(to: point(3, 17), control1: point(8, 16), control2: point(5, 16))
    path.move(to: point(3, 18))
    path.addCurve(to: point(14.8, 5.2), control1: point(7, 13), control2: point(11, 9))
    path.move(to: point(7.2, 12.4))
    path.addLine(to: point(5.2, 10.4))
    path.move(to: point(10.2, 9.2))
    path.addLine(to: point(7.8, 7.2))
    path.move(to: point(11.5, 8))
    path.addLine(to: point(14.2, 8.5))
    path.move(to: point(8.8, 11))
    path.addLine(to: point(11.2, 11.4))
    return path
  }
}
