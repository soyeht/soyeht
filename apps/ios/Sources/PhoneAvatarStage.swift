import SoyehtClient
import SwiftUI

/// The avatar of a workspace on the phone and the iPad: the shared stage on a flat surface of the
/// palette. It only draws what the model gives it; the state of the conversation is the model's
/// to set. A package that did not load says so here, by its kind, in the palette's colours.
struct PhoneAvatarStage: View {
  let model: AvatarStageModel

  var body: some View {
    ZStack {
      AvatarStageView(model: model)
      if let text = model.failureText {
        Text(verbatim: text)
          .font(.footnote)
          .foregroundStyle(PhonePalette.secondary)
          .multilineTextAlignment(.center)
          .padding(DesignTokens.Space.x8)
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(PhonePalette.canvas)
    .accessibilityIdentifier("avatar-stage")
  }
}
