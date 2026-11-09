import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// "Trust this folder?": the question the first time an undecided workspace becomes the active
/// one, and again when the person opens it from the editor's restricted marker. Dismissing it
/// puts the question off until the app starts over (`ProjectTrustReplica`).
struct ProjectTrustSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  @State private var answering = false

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x18) {
      Text(ProjectTrustPresentation.title)
        .font(MacTheme.ui(MacTheme.TypeScale.heading, weight: .semibold))
      if let path = client.workspaces.first(where: { $0.workspaceId == workspaceId })?.path {
        Text(path)
          .font(MacTheme.mono(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.secondary)
          .lineLimit(2).truncationMode(.middle)
      }
      Text(ProjectTrustPresentation.summary)
        .font(MacTheme.ui(MacTheme.TypeScale.control))
        .foregroundStyle(MacTheme.secondary)
      HStack(spacing: MacTheme.Space.x10) {
        Spacer()
        ForEach(ProjectTrustPresentation.choices, id: \.choice) { choice in
          if choice.isDefault {
            Button(choice.label) { answer(choice.choice) }
              .buttonStyle(.panePrimary)
              .keyboardShortcut(.defaultAction)
              .disabled(answering)
          } else {
            Button(choice.label) { answer(choice.choice) }
              .buttonStyle(.paneSecondary)
              .disabled(answering)
          }
        }
      }
    }
    .padding(MacTheme.Space.x18)
    .macSheetSurface(.projectTrust)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("project-trust-sheet")
  }

  private func answer(_ choice: ProjectTrustChoice) {
    answering = true
    Task {
      await client.setProjectTrust(workspaceId: workspaceId, to: choice)
      answering = false
    }
  }
}

private struct ProjectTrustSheetHost: ViewModifier {
  @Bindable var client: EngineClient
  let selectedWorkspaceId: String?

  func body(content: Content) -> some View {
    content.sheet(isPresented: client.projectTrustPrompt(selected: selectedWorkspaceId)) {
      if let id = client.projectTrust.promptedWorkspace(selected: selectedWorkspaceId) {
        ProjectTrustSheet(client: client, workspaceId: id)
      }
    }
  }
}

extension View {
  /// Asks "Trust this folder?" about the workspace on screen while its trust is undecided.
  func projectTrustSheet(client: EngineClient, selectedWorkspaceId: String?) -> some View {
    modifier(ProjectTrustSheetHost(client: client, selectedWorkspaceId: selectedWorkspaceId))
  }
}
