import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/workspaces.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("workspaces") }

private func label(_ item: JsonValue) -> Testing.Comment {
  Testing.Comment(rawValue: item["name"].string ?? "")
}

private func workspaces(_ items: JsonValue) -> [Workspace] {
  (items.array ?? []).map {
    Workspace(
      workspaceId: $0["workspaceId"].string ?? "", name: "", path: $0["path"].string ?? "",
      layout: .null)
  }
}

@Test func aWorkspaceIsNamedForTheLastComponentOfItsFolder() throws {
  for item in ContractVector.cases(try vector(), "names") {
    let path = try #require(item["path"].string)
    #expect(WorkspaceFolderPolicy.name(of: path) == item["workspaceName"].string, label(item))
  }
}

@Test func aTypedPathIsResolvedAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "resolve") {
    let typed = try #require(item["typed"].string)
    switch WorkspaceFolderPolicy.resolve(typed) {
    case .success(let path):
      #expect(path == item["path"].string, label(item))
    case .failure(let problem):
      let name = problem == .empty ? "empty" : "notAbsolute"
      #expect(name == item["problem"].string, label(item))
    }
  }
}

@Test func aFolderThatHasAWorkspaceReusesIt() throws {
  for item in ContractVector.cases(try vector(), "reuse") {
    let path = try #require(item["path"].string)
    let held = workspaces(item["workspaces"])
    #expect(
      WorkspaceFolderPolicy.existingWorkspace(at: path, in: held)?.workspaceId
        == item["reuses"].string,
      label(item))
  }
}

@Test func theRecentFoldersAreTheOnesTheVectorLists() throws {
  for item in ContractVector.cases(try vector(), "recent") {
    let folders = WorkspaceFolderPolicy.recent(
      workspaces: workspaces(item["workspaces"]),
      panes: (item["paneFolders"].array ?? []).compactMap(\.string).map { pane("p", cwd: $0) })
    let expected = item["folders"].array ?? []
    #expect(folders.map(\.path) == expected.map { $0["path"].string ?? "" }, label(item))
    #expect(folders.map(\.name) == expected.map { $0["name"].string ?? "" }, label(item))
    #expect(folders.map(\.workspaceId) == expected.map { $0["workspaceId"].string }, label(item))
  }
}

@Test func aRenameIsSubmittedAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "submission") {
    let typed = try #require(item["typed"].string)
    let current = try #require(item["current"].string)
    #expect(
      WorkspaceNaming.submission(typed, current: current) == item["submitted"].string, label(item))
  }
}

@Test func closingAWorkspaceSelectsTheOneTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "closing") {
    let order = (item["order"].array ?? []).compactMap(\.string)
    let closed = try #require(item["closed"].string)
    #expect(
      WorkspaceTabStripPolicy.selectionAfterClosing(workspaceId: closed, orderedWorkspaceIds: order)
        == item["selected"].string,
      label(item))
  }
}
