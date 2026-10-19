import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The screens that choose a folder on the Mac, pushed from a sheet's navigation stack.
enum PhoneFolderRoute: Hashable {
  case picker
  case browse(WorkspaceFolderBrowser)
}

extension View {
  /// Shows the folder screens a `PhoneFolderRoute` names. `choose` receives the folder's
  /// absolute path on the Mac.
  func phoneFolderDestinations(
    client: EngineClient, selected: String, choose: @escaping (String) -> Void
  ) -> some View {
    navigationDestination(for: PhoneFolderRoute.self) { route in
      switch route {
      case .picker:
        PhoneFolderPicker(client: client, selected: selected, choose: choose)
      case .browse(let browser):
        PhoneFolderBrowser(client: client, browser: browser, selected: selected, choose: choose)
      }
    }
  }
}

extension View {
  /// The page of a creation sheet: the canvas, a title, and Cancel and Create in the bar.
  func phoneCreatePage(
    title: String, canCreate: Bool, identifier: String, create: @escaping () -> Void
  ) -> some View {
    modifier(
      PhoneCreatePage(title: title, canCreate: canCreate, identifier: identifier, create: create))
  }
}

private struct PhoneCreatePage: ViewModifier {
  let title: String
  let canCreate: Bool
  let identifier: String
  let create: () -> Void
  @Environment(\.dismiss) private var dismiss

  func body(content: Content) -> some View {
    content
      .scrollContentBackground(.hidden)
      .background(PhonePalette.canvas)
      .navigationTitle(title)
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { dismiss() }
        }
        ToolbarItem(placement: .confirmationAction) {
          Button("Create", action: create)
            .disabled(!canCreate)
            .accessibilityIdentifier(identifier)
        }
      }
  }
}

/// A folder's name and path, with a check while it is the chosen one.
struct PhoneFolderLabel: View {
  let name: String
  let path: String
  var chosen = false

  var body: some View {
    HStack(spacing: DesignTokens.Space.x10) {
      Image(systemName: "folder").foregroundStyle(PhonePalette.accent)
      VStack(alignment: .leading, spacing: DesignTokens.Space.x2) {
        Text(name).font(.headline).lineLimit(1)
        Text(path).font(.caption).foregroundStyle(PhonePalette.secondary)
          .lineLimit(1).truncationMode(.middle)
      }
      Spacer(minLength: 0)
      if chosen { Image(systemName: "checkmark").foregroundStyle(PhonePalette.accent) }
    }
  }
}

/// The first folder screen: a path to type, and the folders the Mac already works in.
struct PhoneFolderPicker: View {
  let client: EngineClient
  let selected: String
  let choose: (String) -> Void
  @State private var typed = ""
  @State private var problem: FolderPathProblem?

  private var recent: [WorkspaceFolder] {
    WorkspaceFolderPolicy.recent(workspaces: client.workspaces, panes: client.panes)
  }

  var body: some View {
    List {
      Section {
        TextField("/Users/you/project", text: $typed)
          .textInputAutocapitalization(.never).autocorrectionDisabled()
          .onSubmit(useTypedPath)
          .accessibilityIdentifier("folder-path-field")
        Button("Use this path", action: useTypedPath)
          .disabled(typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
      } header: {
        Text("Path on the Mac")
      } footer: {
        if let problem {
          Text(problem.explanation).foregroundStyle(PhonePalette.danger)
        } else {
          Text("The Mac checks that the folder exists.")
        }
      }
      .listRowBackground(PhonePalette.face)
      Section("Folders the Mac works in") {
        if recent.isEmpty {
          Text("No folders yet. Type a path above.").foregroundStyle(PhonePalette.secondary)
        }
        ForEach(recent) { folder in
          if let browser = WorkspaceFolderBrowser(root: folder) {
            NavigationLink(value: PhoneFolderRoute.browse(browser)) {
              PhoneFolderLabel(
                name: folder.name, path: folder.path, chosen: folder.path == selected)
            }
          } else {
            Button {
              choose(folder.path)
            } label: {
              PhoneFolderLabel(
                name: folder.name, path: folder.path, chosen: folder.path == selected)
            }
            .buttonStyle(.plain)
          }
        }
      }
      .listRowBackground(PhonePalette.face)
    }
    .scrollContentBackground(.hidden)
    .background(PhonePalette.canvas)
    .navigationTitle("Choose folder")
    .navigationBarTitleDisplayMode(.inline)
  }

  private func useTypedPath() {
    switch WorkspaceFolderPolicy.resolve(typed) {
    case .success(let path):
      problem = nil
      choose(path)
    case .failure(let failure):
      problem = failure
    }
  }
}

/// A folder inside a workspace of the Mac: use it, or look inside it.
struct PhoneFolderBrowser: View {
  let client: EngineClient
  let browser: WorkspaceFolderBrowser
  let selected: String
  let choose: (String) -> Void
  @State private var listing = Listing.loading

  private enum Listing {
    case loading
    case failed
    case loaded([ProjectEntry])
  }

  var body: some View {
    List {
      Section {
        Button {
          choose(browser.path)
        } label: {
          PhoneFolderLabel(
            name: "Use \(browser.name)", path: browser.path, chosen: browser.path == selected)
        }
        .buttonStyle(.plain)
      }
      .listRowBackground(PhonePalette.face)
      Section("Folders inside") {
        switch listing {
        case .loading:
          ProgressView().frame(maxWidth: .infinity)
        case .failed:
          Text("The Mac did not list this folder.").foregroundStyle(PhonePalette.secondary)
          Button("Try again") { Task { await load() } }
        case .loaded(let entries):
          if entries.isEmpty {
            Text("No folders inside.").foregroundStyle(PhonePalette.secondary)
          }
          ForEach(entries, id: \.path) { entry in
            NavigationLink(value: PhoneFolderRoute.browse(browser.entering(entry))) {
              Label(entry.name, systemImage: "folder")
            }
          }
        }
      }
      .listRowBackground(PhonePalette.face)
    }
    .scrollContentBackground(.hidden)
    .background(PhonePalette.canvas)
    .navigationTitle(browser.name)
    .navigationBarTitleDisplayMode(.inline)
    .task { await load() }
  }

  private func load() async {
    listing = .loading
    do {
      listing = .loaded(try await client.listFolders(in: browser))
    } catch {
      listing = .failed
    }
  }
}
