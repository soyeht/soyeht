import SoyehtClient
import SwiftUI

/// The avatar of a workspace on the phone and the iPad: the shared stage on a flat surface of the
/// palette. It only draws what the model gives it; the state of the conversation is the model's
/// to set.
struct PhoneAvatarStage: View {
  let model: AvatarStageModel

  var body: some View {
    AvatarStageView(model: model)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(PhonePalette.canvas)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel("Avatar")
      .accessibilityIdentifier("avatar-stage")
  }
}
