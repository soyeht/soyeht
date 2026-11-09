import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func folderGone() -> ClientFailure {
  ClientFailure.remote(
    ErrorKind.workspace_folder_missing.rawValue, "The workspace folder is missing")
}

@MainActor private func clientWithWorkspace() -> EngineClient {
  let client = EngineClient(socket: "/tmp/missing-workspace-folder.sock")
  client.upsertWorkspace(
    Workspace(
      workspaceId: "w1", name: "scratch", path: "/tmp/scratch-gone", layout: .object([:])))
  return client
}

@MainActor @Test func aWorkspaceWhoseFolderIsGoneIsAStateAndOpensNoDialog() {
  let client = clientWithWorkspace()

  #expect(client.noteMissingWorkspaceFolder(folderGone(), workspaceId: "w1"))

  #expect(client.problem == nil)
  let state = client.missingWorkspaceFolder(in: "w1")
  #expect(state?.text == "\(ClientPolicy.workspaceFolderGone) /tmp/scratch-gone")
  #expect(state?.closeTitle == "Close workspace")
  #expect(client.missingWorkspaceFolder(in: "w2") == nil)
}

@MainActor @Test func anyOtherFailureIsNotTheStateOfAMissingFolder() {
  let client = clientWithWorkspace()

  for failure in [
    ClientFailure.remote(ErrorKind.internal_error.rawValue, "I/O operation failed"),
    ClientFailure.remote(ErrorKind.not_found.rawValue, "No such folder"),
    ClientFailure.disconnected,
  ] {
    #expect(!client.noteMissingWorkspaceFolder(failure, workspaceId: "w1"))
  }
  #expect(client.missingWorkspaceFolder(in: "w1") == nil)
}

@MainActor @Test func aFailedOpenInAWorkspaceWithNoFolderIsTheStateNotTheDialog() {
  let client = clientWithWorkspace()

  client.reportEditorOpenFailure(folderGone(), workspaceId: "w1", path: "src/lib.rs")

  #expect(client.problem == nil)
  #expect(client.editorOpenRefusal(in: "w1") == nil)
  #expect(client.missingWorkspaceFolder(in: "w1") != nil)
}

@MainActor @Test func aClosedWorkspaceTakesItsStateWithIt() {
  let client = clientWithWorkspace()
  client.noteMissingWorkspaceFolder(folderGone(), workspaceId: "w1")

  client.applyWorkspaceClosed(.object(["workspaceId": .string("w1")]))

  #expect(client.missingWorkspaceFolder(in: "w1") == nil)
}
