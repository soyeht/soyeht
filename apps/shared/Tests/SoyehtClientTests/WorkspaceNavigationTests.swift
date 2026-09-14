import SoyehtProtocol
import Testing

@testable import SoyehtClient

private final class MemoryClientPreferenceStore: ClientPreferenceStore {
  private var values: [String: Bool] = [:]

  func bool(forKey key: String) -> Bool {
    values[key] ?? false
  }

  func set(_ value: Bool, forKey key: String) {
    values[key] = value
  }
}

@Test func writingDestinationsMapToOneClosedSectionAndKind() {
  #expect(WorkspaceDestination.texts.writingSection == .texts)
  #expect(WorkspaceDestination.presentations.writingSection == .presentations)
  #expect(WorkspaceDestination.screenplays.writingSection == .screenplays)
  #expect(WorkspaceDestination.editor.writingSection == nil)
  #expect(WritingSection.texts.destination == .texts)
  #expect(WritingSection.presentations.writingKind == .presentation)
  #expect(WritingSection.screenplays.writingKind == .screenplay)
}

@Test
@MainActor
func appBarVisibilityPersistsWithoutPersistingTheSelectedView() {
  let preferences = MemoryClientPreferenceStore()
  let navigation = WorkspaceNavigation(preferences: preferences)

  #expect(navigation.appBarOpen == false)
  #expect(navigation.destination == .agents)

  navigation.toggleAppBar()
  navigation.select(.web)

  #expect(navigation.appBarOpen)
  #expect(navigation.destination == .web)

  let restored = WorkspaceNavigation(preferences: preferences)
  #expect(restored.appBarOpen)
  #expect(restored.destination == .agents)
}
