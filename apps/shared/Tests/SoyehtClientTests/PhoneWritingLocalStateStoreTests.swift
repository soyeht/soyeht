import Foundation
import Testing

@testable import SoyehtClient

@Test func phoneWritingStateStoreNamespacesAndRestoresDeviceLocalState() throws {
  let preferences = MemoryPhoneWritingPreferences()
  let store = PhoneWritingLocalStateStore(preferences: preferences)
  let state = PhoneWritingLocalState(
    panes: [
      PhoneWritingPaneState(
        identity: PhoneWritingIdentity(paneId: "pane", fileId: "file"), expanded: true)
    ])

  try store.save(state, engineId: "engine-a", workspaceId: "workspace")

  #expect(store.load(engineId: "engine-a", workspaceId: "workspace") == state)
  #expect(store.load(engineId: "engine-b", workspaceId: "workspace") == nil)
  #expect(store.load(engineId: "engine-a", workspaceId: "other") == nil)
  #expect(preferences.values.count == 1)
}

@Test func phoneWritingStateStoreDropsMalformedAndUnknownVersions() {
  let preferences = MemoryPhoneWritingPreferences()
  let store = PhoneWritingLocalStateStore(preferences: preferences)
  let malformedKey = store.key(engineId: "engine", workspaceId: "malformed")
  let unknownKey = store.key(engineId: "engine", workspaceId: "unknown")
  preferences.values[malformedKey] = Data("not json".utf8)
  preferences.values[unknownKey] = Data("{\"version\":99,\"panes\":[]}".utf8)

  #expect(store.load(engineId: "engine", workspaceId: "malformed") == nil)
  #expect(store.load(engineId: "engine", workspaceId: "unknown") == nil)
  #expect(preferences.values[malformedKey] == nil)
  #expect(preferences.values[unknownKey] == nil)
}

@Test func phoneWritingStateStoreRemovesOnlyTheRequestedWorkspace() throws {
  let preferences = MemoryPhoneWritingPreferences()
  let store = PhoneWritingLocalStateStore(preferences: preferences)
  let state = PhoneWritingLocalState()
  try store.save(state, engineId: "engine", workspaceId: "first")
  try store.save(state, engineId: "engine", workspaceId: "second")

  store.remove(engineId: "engine", workspaceId: "first")

  #expect(store.load(engineId: "engine", workspaceId: "first") == nil)
  #expect(store.load(engineId: "engine", workspaceId: "second") == state)
}

@Test func phoneWritingStateOpeningNewDocumentPreservesSiblingsAndExpandsTheNewLeaf() {
  let sibling = PhoneWritingPaneState(
    identity: PhoneWritingIdentity(paneId: "existing", fileId: "file-existing"),
    expanded: true, activeTab: .agent, screenplayZoom: 2)
  let staleIdentity = PhoneWritingPaneState(
    identity: PhoneWritingIdentity(paneId: "new", fileId: nil), expanded: false)
  let identity = PhoneWritingIdentity(paneId: "new", fileId: "file-new")

  let opened = PhoneWritingLocalState(panes: [sibling, staleIdentity]).opening(identity)

  #expect(opened.panes == [sibling, PhoneWritingPaneState(identity: identity, expanded: true)])
}

private final class MemoryPhoneWritingPreferences: PhoneWritingLocalStatePreferences {
  var values: [String: Data] = [:]

  func data(forKey key: String) -> Data? { values[key] }
  func set(_ value: Data?, forKey key: String) { values[key] = value }
}
