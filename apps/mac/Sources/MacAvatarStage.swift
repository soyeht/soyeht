import SoyehtClient
import SwiftUI

/// The avatar of a workspace on the Mac: the shared stage on a flat surface of the palette. It
/// only draws what the model gives it; the state of the conversation is the model's to set.
struct MacAvatarStage: View {
  let model: AvatarStageModel

  var body: some View {
    AvatarStageView(model: model)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(MacTheme.canvas)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel("Avatar")
      .accessibilityIdentifier("avatar-stage")
  }
}
