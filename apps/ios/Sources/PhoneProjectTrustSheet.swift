import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// "Trust this folder?": asked the first time an undecided workspace is the one on screen.
/// Dismissing it puts the question off until the app starts over (`ProjectTrustReplica`).
struct PhoneProjectTrustSheet: View {
  let client: EngineClient
  let workspaceId: String
  @State private var answering = false

  var body: some View {
    VStack(alignment: .leading, spacing: DesignTokens.Space.x16) {
      Text(ProjectTrustPresentation.title).font(.headline)
      if let path = client.workspaces.first(where: { $0.workspaceId == workspaceId })?.path {
        Text(path).font(.footnote.monospaced()).foregroundStyle(PhonePalette.secondary)
          .lineLimit(2).truncationMode(.middle)
      }
      Text(ProjectTrustPresentation.summary).font(.subheadline)
        .foregroundStyle(PhonePalette.secondary)
      ForEach(ProjectTrustPresentation.choices, id: \.choice) { choice in
        if choice.isDefault {
          Button(choice.label) { answer(choice.choice) }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("project-trust-restricted")
        } else {
          Button(choice.label) { answer(choice.choice) }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("project-trust-trusted")
        }
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(DesignTokens.Space.x18)
    .disabled(answering)
    .presentationDetents([.medium])
    .tint(PhonePalette.accent)
  }

  private func answer(_ choice: ProjectTrustChoice) {
    answering = true
    Task {
      await client.setProjectTrust(workspaceId: workspaceId, to: choice)
      answering = false
    }
  }
}

extension View {
  /// Asks "Trust this folder?" about the workspace on screen while its trust is undecided.
  func projectTrustSheet(client: EngineClient) -> some View {
    let selected = client.selectedPane?.workspaceId
    return sheet(isPresented: client.projectTrustPrompt(selected: selected)) {
      if let id = client.projectTrust.promptedWorkspace(selected: selected) {
        PhoneProjectTrustSheet(client: client, workspaceId: id)
      }
    }
  }
}
