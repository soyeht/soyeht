import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func listing(_ path: String, _ entries: [(String, Bool)], ignored: Set<String> = [])
  -> ProjectTreeListResult
{
  ProjectTreeListResult(
    workspaceId: "workspace-1", path: path,
    entries: entries.map { name, folder in
      ProjectEntry(
        name: name, path: EditorProjectTree.join(path, name),
        kind: folder ? .directory : .file, ignored: ignored.contains(name))
    },
    truncated: false)
}

/// root: src/ (lib.rs, ui/ (view.swift)), README.md
private func loadedTree() -> EditorProjectTree {
  var tree = EditorProjectTree()
  tree.apply(listing("", [("src", true), ("README.md", false)]))
  tree.expand("src")
  tree.apply(listing("src", [("ui", true), ("lib.rs", false)]))
  return tree
}

private func paths(_ tree: EditorProjectTree) -> [String] { tree.rows.map(\.node.path) }

@Test func treeListsOnlyExpandedFoldersAndAsksForMissingListings() {
  var tree = EditorProjectTree()
  #expect(tree.directoriesToLoad == [""])
  tree = loadedTree()
  #expect(paths(tree) == ["src", "src/ui", "src/lib.rs", "README.md"])
  #expect(tree.rows.map(\.depth) == [0, 1, 1, 0])
  #expect(tree.directoriesToLoad.isEmpty)
  #expect(tree.watchedDirectories == ["", "src"])

  let needsListing = tree.toggle("src/ui")
  #expect(needsListing)
  #expect(tree.directoriesToLoad == ["src/ui"])
  tree.apply(listing("src/ui", [("view.swift", false)]))
  #expect(paths(tree) == ["src", "src/ui", "src/ui/view.swift", "src/lib.rs", "README.md"])

  tree.collapse("src")
  #expect(paths(tree) == ["src", "README.md"])
  // A collapsed folder is no longer watched, so reopening it lists it again
  // and restores the nested folder that was open.
  #expect(tree.watchedDirectories == [""])
  let reopened = tree.expand("src")
  #expect(reopened)
  #expect(tree.directoriesToLoad == ["src"])
  tree.apply(listing("src", [("ui", true), ("lib.rs", false)]))
  #expect(tree.directoriesToLoad == ["src/ui"])
  tree.collapse(EditorProjectTree.root)
  #expect(!tree.rows.isEmpty)
}

@Test func relistingDropsVanishedEntriesWithTheirExpansionAndSelection() {
  var tree = loadedTree()
  tree.expand("src/ui")
  tree.apply(listing("src/ui", [("view.swift", false)]))
  tree.selection = "src/ui/view.swift"

  tree.apply(listing("src", [("lib.rs", false), ("main.rs", false)]))
  #expect(paths(tree) == ["src", "src/lib.rs", "src/main.rs", "README.md"])
  #expect(tree.selection == nil)
  #expect(!tree.expanded.contains("src/ui"))
  #expect(tree.children["src/ui"] == nil)
}

@Test func revealExpandsEveryAncestorAndSelectsTheFile() {
  var tree = EditorProjectTree()
  tree.apply(listing("", [("src", true)]))
  let pending = tree.reveal("src/ui/view.swift")
  #expect(pending == ["src"])
  #expect(tree.selection == "src/ui/view.swift")
  #expect(EditorProjectTree.ancestors(of: "src/ui/view.swift") == ["src/ui", "src", ""])
}

@Test func keyboardMovesLikeZedsProjectPanel() {
  var tree = loadedTree()
  tree.handle(.down)
  #expect(tree.selection == "src")
  tree.handle(.right)
  #expect(tree.selection == "src/ui")
  // Right on a collapsed folder expands it and asks for its listing.
  let listingNeeded = tree.handle(.right)
  #expect(listingNeeded == "src/ui")
  tree.apply(listing("src/ui", [("view.swift", false)]))
  tree.handle(.right)
  #expect(tree.selection == "src/ui/view.swift")
  tree.handle(.left)
  #expect(tree.selection == "src/ui")
  tree.handle(.left)
  #expect(!tree.expanded.contains("src/ui"))
  tree.handle(.down)
  tree.handle(.down)
  tree.handle(.down)
  #expect(tree.selection == "README.md")
  tree.handle(.down)
  #expect(tree.selection == "README.md")
  tree.handle(.first)
  #expect(tree.selection == "src")
  tree.handle(.last)
  #expect(tree.selection == "README.md")
  tree.handle(.up)
  #expect(tree.selection == "src/lib.rs")
  #expect(tree.creationDirectory == "src")
}

@Test func showingIgnoredFilesRelistsEveryExpandedFolder() {
  var tree = loadedTree()
  tree.setShowIgnored(true)
  #expect(tree.showIgnored)
  #expect(tree.rows.isEmpty)
  #expect(tree.directoriesToLoad == [""])
  tree.apply(listing("", [("build", true), ("src", true)], ignored: ["build"]))
  #expect(tree.rows.first?.node.ignored == true)
}
