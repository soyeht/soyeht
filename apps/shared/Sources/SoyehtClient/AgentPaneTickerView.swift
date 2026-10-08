import SoyehtProtocol
import SwiftUI

/// Which source feeds the pane headers' tickers. The ACP source is the default; a summarizer
/// replaces it with `.environment(\.agentTickerSource, …)` and the headers stay as they are.
extension EnvironmentValues {
  @Entry public var agentTickerSource: any AgentTickerSource = AgentACPTickerSource()
}

/// Header metrics of the approved agent pane ("Cabeçalho" in design/soyeht-desktop.pen):
/// the ticker line is 18 points tall, ends 12 points before the status and rolls left.
public enum AgentTickerMetrics {
  public static let lineHeight: CGFloat = 18
  public static let trailingPadding: CGFloat = 12
  public static let itemSeparator = "   •   "
  /// Points per second of the rolling line, slow enough to read.
  static let speed: CGFloat = 24
  /// Seconds each entry stays when Reduce Motion replaces rolling with rotating.
  static let rotation: TimeInterval = 6
}

/// The agent pane header's "what is happening" line, fed by `agentTickerSource`. It rolls
/// left when it does not fit, pauses while the pointer is over it or a finger holds it, and
/// with Reduce Motion it shows one entry at a time instead.
public struct AgentPaneTickerView: View {
  @Bindable private var client: EngineClient
  private let paneId: String
  private let font: Font
  private let timeFont: Font
  private let color: Color
  @Environment(\.agentTickerSource) private var source
  @State private var clock = AgentTurnClock()

  public init(client: EngineClient, paneId: String, font: Font, timeFont: Font, color: Color) {
    self.client = client
    self.paneId = paneId
    self.font = font
    self.timeFont = timeFont
    self.color = color
  }

  private var pane: Pane? { client.panes.first { $0.paneId == paneId } }

  private func context(now: Date) -> AgentTickerContext {
    AgentTickerContext(
      pane: pane, view: client.agentPanes[paneId] ?? AgentPaneView(),
      turnStartedAt: clock.startedAt, now: now)
  }

  public var body: some View {
    let activePromptId = pane?.agent?.activePromptId
    let snapshot = source.snapshot(for: context(now: .now))
    HStack(spacing: PaneInteriorMetrics.tightSpacing) {
      AgentTickerLine(items: snapshot.items, font: font, timeFont: timeFont, color: color)
      // Only a running turn ticks, and only its time: an idle pane never re-renders on a timer.
      if snapshot.elapsed != nil {
        TimelineView(.periodic(from: .now, by: 1)) { timeline in
          if let elapsed = source.snapshot(for: context(now: timeline.date)).elapsed {
            Text("· \(elapsed)").font(timeFont).foregroundStyle(color).fixedSize()
              .accessibilityLabel("Turn running for \(elapsed)")
          }
        }
      }
    }
    .padding(.trailing, AgentTickerMetrics.trailingPadding)
    .onAppear { clock.observe(activePromptId: activePromptId, now: .now) }
    .onChange(of: activePromptId) { _, id in clock.observe(activePromptId: id, now: .now) }
    .accessibilityIdentifier("agent-ticker-\(paneId)")
  }
}

/// The rolling line itself. Only a changed text restarts it, so the elapsed time ticking
/// beside it never makes it jump.
struct AgentTickerLine: View {
  let items: [AgentTickerItem]
  let font: Font
  let timeFont: Font
  let color: Color
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var cycleWidth: CGFloat = 0
  @State private var paused = false
  @State private var offset: CGFloat = 0

  private var key: String { items.map { "\($0.time ?? "")\($0.text)" }.joined(separator: "\n") }

  var body: some View {
    GeometryReader { proxy in
      Group {
        if reduceMotion {
          rotating
        } else {
          rolling(width: proxy.size.width)
        }
      }
      .frame(width: proxy.size.width, height: proxy.size.height, alignment: .leading)
      .clipped()
    }
    .frame(height: AgentTickerMetrics.lineHeight)
    .frame(maxWidth: .infinity)
    .contentShape(Rectangle())
    #if os(macOS)
      .onHover { paused = $0 }
    #else
      .onLongPressGesture(minimumDuration: 0.2, pressing: { paused = $0 }, perform: {})
    #endif
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(items.map(\.text).joined(separator: ". "))
  }

  /// Rolls when one cycle (the entries and the separator before the repeat) is wider than
  /// the line; a line that fits stands still without the separator.
  private func rolling(width: CGFloat) -> some View {
    let rolls = cycleWidth > width && !paused
    return HStack(spacing: 0) {
      if rolls {
        cycle
        cycle
      } else {
        text(separated: false)
      }
    }
    .fixedSize()
    .background(alignment: .leading) {
      cycle.fixedSize().hidden()
        .background(
          GeometryReader { text in
            Color.clear.onAppear { cycleWidth = text.size.width }
              .onChange(of: text.size.width) { _, value in cycleWidth = value }
          })
    }
    .offset(x: rolls ? offset : 0)
    .task(id: RollKey(text: key, rolls: rolls, width: cycleWidth)) {
      offset = 0
      guard rolls else { return }
      let distance = cycleWidth
      withAnimation(
        .linear(duration: Double(distance / AgentTickerMetrics.speed)).repeatForever(
          autoreverses: false)
      ) { offset = -distance }
    }
  }

  private var rotating: some View {
    TimelineView(.periodic(from: .now, by: AgentTickerMetrics.rotation)) { timeline in
      let index =
        items.isEmpty
        ? 0
        : Int(timeline.date.timeIntervalSinceReferenceDate / AgentTickerMetrics.rotation)
          % items.count
      if items.indices.contains(index) {
        entry(items[paused ? 0 : index]).lineLimit(1).truncationMode(.tail)
      }
    }
  }

  private var cycle: some View { text(separated: true) }

  /// The entries joined into one line; `separated` ends it with the separator so the copy
  /// that follows while rolling reads on.
  private func text(separated: Bool) -> some View {
    let separator = Text(AgentTickerMetrics.itemSeparator).font(font)
    let joined = items.enumerated().reduce(Text("")) { text, element in
      text + (element.offset == 0 ? Text("") : separator) + entry(element.element)
    }
    return (separated ? joined + separator : joined)
      .foregroundStyle(color).lineLimit(1).fixedSize()
  }

  private func entry(_ item: AgentTickerItem) -> Text {
    let time = item.time.map { Text($0 + "  ").font(timeFont) } ?? Text("")
    return (time + Text(item.text).font(font)).foregroundStyle(color)
  }
}

private struct RollKey: Equatable {
  let text: String
  let rolls: Bool
  let width: CGFloat
}
