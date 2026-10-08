import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The agent composer's harness chip (logo and "Claude ▾"), the way Zed names the agent in
/// its panel. It opens the switch popover: installed harnesses first, then model and effort,
/// then the button that hands the conversation over.
struct PaneHarnessControl: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let agent: AgentSession
  @State private var presented = false

  private var control: HarnessSwitchControl {
    HarnessSwitchModel.control(
      current: agent.harness, switchState: client.agentPanes[pane.paneId]?.harnessSwitch,
      harnesses: client.harnesses, paneName: pane.name)
  }

  var body: some View {
    let control = control
    Button {
      presented = true
    } label: {
      AgentHarnessChipLabel(
        control: control, harnessId: agent.harness, style: MacTheme.agentConversation)
    }
    .buttonStyle(.plain)
    .fixedSize()
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
