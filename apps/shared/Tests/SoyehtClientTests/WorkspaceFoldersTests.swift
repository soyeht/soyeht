import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func workspace(_ id: String, _ path: String, name: String? = nil) -> Workspace {
  Workspace(
    workspaceId: id, name: name ?? WorkspaceFolderPolicy.name(of: path), path: path,
    layout: .object([:]))
}

private func pane(_ id: String, cwd: String) -> Pane {
  Pane(
    paneId: id, workspaceId: "w1", name: id, cwd: cwd, mode: .terminal, state: .idle,
    source: .protocol, confidence: 1, phrase: "idle", generation: 1, cols: 80, rows: 24,
    record: false)
}

@Test func recentFoldersListWorkspaceRootsFirstThenPaneFoldersOnce() {
  let folders = WorkspaceFolderPolicy.recent(
    workspaces: [workspace("w1", "/Users/me/app"), workspace("w2", "/Users/me/site")],
    panes: [
      pane("a", cwd: "/Users/me/app"), pane("b", cwd: "/Users/me/app/tools"),
      pane("c", cwd: "/Users/me/app/tools"), pane("d", cwd: ""),
    ])

  #expect(folders.map(\.path) == ["/Users/me/app", "/Users/me/site", "/Users/me/app/tools"])
  #expect(folders.map(\.workspaceId) == ["w1", "w2", nil])
  #expect(folders.map(\.name) == ["app", "site", "tools"])
}

@Test func aTypedPathIsTrimmedAndMustBeAbsolute() {
  #expect(WorkspaceFolderPolicy.resolve("  /Users/me/app/  ") == .success("/Users/me/app"))
  #expect(WorkspaceFolderPolicy.resolve("/") == .success("/"))
  #expect(WorkspaceFolderPolicy.resolve("C:\\work\\app\\") == .success("C:\\work\\app"))
  #expect(WorkspaceFolderPolicy.resolve("C:\\") == .success("C:\\"))
  #expect(WorkspaceFolderPolicy.resolve("   ") == .failure(.empty))
  #expect(WorkspaceFolderPolicy.resolve("~/app") == .failure(.notAbsolute))
  #expect(WorkspaceFolderPolicy.resolve("app") == .failure(.notAbsolute))
  #expect(FolderPathProblem.notAbsolute.explanation.contains("full path"))
}

@Test func aFolderNameIsItsLastComponent() {
  #expect(WorkspaceFolderPolicy.name(of: "/Users/me/app") == "app")
  #expect(WorkspaceFolderPolicy.name(of: "C:\\work\\app") == "app")
  #expect(WorkspaceFolderPolicy.name(of: "/") == "/")
}

@Test func renamingNeedsADifferentNonBlankName() {
  #expect(WorkspaceNaming.submission("  Site  ", current: "app") == "Site")
  #expect(WorkspaceNaming.submission("app", current: "app") == nil)
  #expect(WorkspaceNaming.submission("   ", current: "app") == nil)
}

@Test func browsingStartsAtAWorkspaceRootAndWalksDownAndBackUp() throws {
  #expect(WorkspaceFolderBrowser(root: WorkspaceFolder(path: "/Users/me/loose")) == nil)
  let root = try #require(
    WorkspaceFolderBrowser(root: WorkspaceFolder(path: "/Users/me/app", workspaceId: "w1")))
  #expect(root.path == "/Users/me/app")
  #expect(root.leaving() == nil)

  let packages = root.entering(
    ProjectEntry(name: "packages", path: "packages", kind: .directory, ignored: false))
  let web = packages.entering(
    ProjectEntry(name: "web", path: "packages/web", kind: .directory, ignored: false))

  #expect(web.path == "/Users/me/app/packages/web")
  #expect(web.name == "web")
  #expect(web.leaving()?.path == "/Users/me/app/packages")
  #expect(web.leaving()?.leaving() == root)
}

@Test func aFolderListingKeepsOnlyVisibleFoldersInNameOrder() {
  let listing = ProjectTreeListResult(
    workspaceId: "w1", path: "",
    entries: [
      ProjectEntry(name: "src10", path: "src10", kind: .directory, ignored: false),
      ProjectEntry(name: "README.md", path: "README.md", kind: .file, ignored: false),
      ProjectEntry(name: "target", path: "target", kind: .directory, ignored: true),
      ProjectEntry(name: "src2", path: "src2", kind: .directory, ignored: false),
    ], truncated: false)

  #expect(WorkspaceFolderBrowser.folders(in: listing).map(\.name) == ["src2", "src10"])
}

@MainActor @Test func creatingAWorkspaceForAKnownFolderReusesIt() async throws {
  let client = EngineClient(socket: "/tmp/workspace-reuse.sock")
  client.upsertWorkspace(workspace("w1", "/Users/me/app"))
  client.requestStub = { _, _ in throw ClientFailure.invalidFrame }

  let result = try await client.createWorkspace(path: "/Users/me/app", name: "Other")

  #expect(result.workspaceId == "w1")
  #expect(client.workspaces.count == 1)
}

@MainActor @Test func creatingRenamingAndListingGoThroughTheEngine() async throws {
  let client = EngineClient(socket: "/tmp/workspace-actions.sock")
  var requests: [(RequestMethod, JsonValue)] = []
  client.requestStub = { method, params in
    requests.append((method, params))
    switch method {
    case .workspaceCreate:
      return try .object(["workspace": .encoded(workspace("w9", "/Users/me/site", name: "site"))])
    case .workspaceRename:
      return try .object(["workspace": .encoded(workspace("w9", "/Users/me/site", name: "Site"))])
    case .projectTreeList:
      return try .encoded(
        ProjectTreeListResult(
          workspaceId: "w9", path: "docs",
          entries: [
            ProjectEntry(name: "guides", path: "docs/guides", kind: .directory, ignored: false)
          ], truncated: false))
    default: throw ClientFailure.invalidFrame
    }
  }

  let created = try await client.createWorkspace(path: "/Users/me/site")
  #expect(created.name == "site")
  #expect(client.workspaces.map(\.workspaceId) == ["w9"])
  #expect(requests[0].1["name"].string == "site")
  #expect(requests[0].1["path"].string == "/Users/me/site")

  try await client.renameWorkspace("w9", to: "Site")
  #expect(client.workspaces.first?.name == "Site")
  #expect(requests[1].1["workspaceId"].string == "w9")
  #expect(requests[1].1["name"].string == "Site")

  let root = try #require(
    WorkspaceFolderBrowser(root: WorkspaceFolder(path: "/Users/me/site", workspaceId: "w9")))
  let docs = root.entering(
    ProjectEntry(name: "docs", path: "docs", kind: .directory, ignored: false))
  let folders = try await client.listFolders(in: docs)
  #expect(folders.map(\.path) == ["docs/guides"])
  #expect(requests[2].1["path"].string == "docs")
}
