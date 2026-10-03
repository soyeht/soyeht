import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The agent pane's harness, named on a menu button in its header ("Claude ▾"), the way Zed
/// names the agent in its panel. It opens the switch popover: installed harnesses first, then
/// model and effort, then the button that hands the conversation over.
struct PaneHarnessControl: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let control: HarnessSwitchControl
  @State private var presented = false

  var body: some View {
    Button {
      presented = true
    } label: {
      HStack(spacing: MacTheme.Space.x3) {
        if control.busy {
          ProgressView().controlSize(.mini)
        }
        Text(control.title)
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(MacTheme.ink)
          .lineLimit(1).truncationMode(.tail)
        Image(systemName: HarnessSwitchControl.symbol)
          .imageScale(.small)
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      }
      .padding(.horizontal, MacTheme.Space.x8)
      .frame(
        height: CGFloat(PaneLayoutContentMetrics.genericPaneHeaderHeight) - MacTheme.Space.x6
      )
      .background(MacTheme.well, in: Capsule())
      .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .frame(
      maxWidth: PaneLayoutContentMetrics.genericPaneHeaderHarnessControlWidth,
      alignment: .trailing
    )
    .fixedSize(horizontal: false, vertical: true)
    .layoutPriority(2)
    .help(control.help)
    .accessibilityLabel(HarnessSwitchControl.accessibilityLabel)
    .accessibilityValue(control.title)
    .accessibilityHint(HarnessSwitchControl.accessibilityHint)
    .accessibilityIdentifier("switch-harness-\(pane.paneId)")
    .disabled(!client.canSwitchAgentHarness(pane.paneId))
    .popover(isPresented: $presented, arrowEdge: .bottom) {
      AgentHarnessSwitchPicker(client: client, pane: pane, isPresented: $presented)
    }
  }
}
