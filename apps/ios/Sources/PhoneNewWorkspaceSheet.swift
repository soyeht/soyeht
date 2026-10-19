import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// Opens a workspace for a folder on the Mac. A folder that already has one gets that one.
struct PhoneNewWorkspaceSheet: View {
  let client: EngineClient
  let created: (Workspace) -> Void
  @State private var route = NavigationPath()
  @State private var path = ""
  @State private var name = ""
  @State private var creating = false
  @State private var failure: String?

  private var existing: Workspace? {
    WorkspaceFolderPolicy.existingWorkspace(at: path, in: client.workspaces)
  }

  var body: some View {
    NavigationStack(path: $route) {
      Form {
        Section("Folder on the Mac") {
          NavigationLink(value: PhoneFolderRoute.picker) {
            if path.isEmpty {
              Label("Choose a folder", systemImage: "folder.badge.plus")
            } else {
              PhoneFolderLabel(name: WorkspaceFolderPolicy.name(of: path), path: path)
            }
          }
          .accessibilityIdentifier("workspace-folder")
        }
        .listRowBackground(PhonePalette.face)
        Section {
          TextField(
            path.isEmpty ? "Name" : WorkspaceFolderPolicy.name(of: path), text: $name
          )
          .disabled(existing != nil)
        } header: {
          Text("Name")
        } footer: {
          if let existing {
            Text("This folder already has the workspace \(existing.name). It opens that one.")
          } else {
            Text("Leave it empty to name the workspace after its folder.")
          }
        }
        .listRowBackground(PhonePalette.face)
        if let failure {
          Section { Text(failure).foregroundStyle(PhonePalette.danger) }
            .listRowBackground(PhonePalette.face)
        }
      }
      .phoneCreatePage(
        title: "New workspace", canCreate: !path.isEmpty && !creating,
        identifier: "workspace-create", create: create
      )
      .phoneFolderDestinations(client: client, selected: path) { chosen in
        path = chosen
        route = NavigationPath()
      }
    }
    .tint(PhonePalette.accent)
    .foregroundStyle(PhonePalette.ink)
  }

  private func create() {
    guard !path.isEmpty, !creating else { return }
    let typedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
    creating = true
    failure = nil
    Task { @MainActor in
      defer { creating = false }
      do {
        let workspace = try await client.createWorkspace(
          path: path, name: typedName.isEmpty ? nil : typedName)
        created(workspace)
      } catch {
        failure = error.localizedDescription
      }
    }
  }
}
