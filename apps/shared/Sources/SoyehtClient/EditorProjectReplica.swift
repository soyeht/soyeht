import Foundation
import SoyehtProtocol

/// Project panel operations. Every filesystem change goes through the engine,
/// which confines it to the workspace; a paired iPhone or iPad has the same
/// powers as the person at the Mac.
extension EngineClient {
  public func projectTree(_ workspaceId: String) -> EditorProjectTree {
    editorWorkspace.trees[workspaceId] ?? EditorProjectTree()
  }

  /// Changes the panel tree, then lists the folders it now shows and updates
  /// the engine's watch on them.
  public func updateProjectTree(
    _ workspaceId: String, _ change: (inout EditorProjectTree) -> Void
  ) async {
    var tree = projectTree(workspaceId)
    change(&tree)
    editorWorkspace.trees[workspaceId] = tree
    await synchronizeProjectTree(workspaceId)
  }

  /// Lists every expanded folder that has no listing and sends the watch set.
  /// On a new connection it relists what is shown and watches it again,
  /// because watches belong to a connection.
  public func synchronizeProjectTree(_ workspaceId: String) async {
    guard connected else { return }
    if editorWorkspace.treeConnection[workspaceId] != connectionRevision {
      editorWorkspace.treeConnection[workspaceId] = connectionRevision
      editorWorkspace.watched[workspaceId] = nil
      await refreshProjectDirectories(
        workspaceId, paths: projectTree(workspaceId).watchedDirectories)
    }
    while let directory = projectTree(workspaceId).directoriesToLoad.first {
      do {
        try await loadProjectDirectory(workspaceId, path: directory)
      } catch is CancellationError {
        return
      } catch {
        guard directory != EditorProjectTree.root else {
          problem = error.localizedDescription
          return
        }
        // A folder that disappeared before it was listed simply closes.
        var tree = projectTree(workspaceId)
        tree.collapse(directory)
        editorWorkspace.trees[workspaceId] = tree
      }
    }
    do {
      let watched = projectTree(workspaceId).watchedDirectories
      guard editorWorkspace.watched[workspaceId] != watched else { return }
      _ = try await request(
        "project.tree.watch",
        .encoded(ProjectTreeWatchParams(workspaceId: workspaceId, paths: watched)))
      editorWorkspace.watched[workspaceId] = watched
    } catch is CancellationError {
      return
    } catch {
      problem = error.localizedDescription
    }
  }

  /// Relists folders the engine reported as changed, or that this client changed.
  public func refreshProjectDirectories(_ workspaceId: String, paths: [String]) async {
    let listed = Set(projectTree(workspaceId).watchedDirectories)
    for path in paths where listed.contains(path) {
      do {
        try await loadProjectDirectory(workspaceId, path: path)
      } catch {
        // A folder that vanished is dropped by its parent's next listing.
        if projectTree(workspaceId).watchedDirectories.contains(path) {
          editorWorkspace.trees[workspaceId]?.collapse(path)
        }
      }
    }
  }

  @discardableResult
  public func createProjectEntry(
    workspaceId: String, path: String, kind: ProjectEntryKind
  ) async throws -> ProjectEntry {
    let result = try await mutation(
      "project.entry.create",
      ProjectEntryCreateParams(
        workspaceId: workspaceId, path: path, kind: kind, idempotencyKey: UUID().uuidString)
    ).decoded(ProjectEntryCreateResult.self)
    await refreshProjectDirectories(
      workspaceId, paths: EditorProjectTree.ancestors(of: result.entry.path))
    await revealProjectEntry(workspaceId, path: result.entry.path)
    return result.entry
  }

  public func renameProjectEntry(workspaceId: String, from: String, to: String) async throws {
    let result = try await mutation(
      "project.entry.rename",
      ProjectEntryRenameParams(
        workspaceId: workspaceId, from: from, to: to, idempotencyKey: UUID().uuidString)
    ).decoded(ProjectEntryRenameResult.self)
    for file in result.files { upsertEditor(file) }
    await refreshProjectDirectories(
      workspaceId,
      paths: [EditorProjectTree.parent(of: from), EditorProjectTree.parent(of: to)])
    await revealProjectEntry(workspaceId, path: result.to)
  }

  /// Moves an entry to the system trash. When the engine host has no trash the
  /// result is `.needs_confirmation` and nothing changed; the caller asks the
  /// person and calls again with `permanent`.
  public func deleteProjectEntry(
    workspaceId: String, path: String, permanent: Bool = false
  ) async throws -> ProjectDeleteOutcome {
    let result = try await mutation(
      "project.entry.delete",
      ProjectEntryDeleteParams(
        workspaceId: workspaceId, path: path, permanent: permanent,
        idempotencyKey: UUID().uuidString)
    ).decoded(ProjectEntryDeleteResult.self)
    if result.outcome != .needs_confirmation {
      await refreshProjectDirectories(workspaceId, paths: [EditorProjectTree.parent(of: path)])
    }
    return result.outcome
  }

  /// Expands the folders above `path` and selects it.
  public func revealProjectEntry(_ workspaceId: String, path: String) async {
    await updateProjectTree(workspaceId) { $0.reveal(path) }
  }

  public func setProjectTreeShowsIgnored(_ workspaceId: String, _ show: Bool) async {
    await updateProjectTree(workspaceId) { $0.setShowIgnored(show) }
  }

  private func loadProjectDirectory(_ workspaceId: String, path: String) async throws {
    let showIgnored = projectTree(workspaceId).showIgnored
    let listing = try await request(
      "project.tree.list",
      .encoded(
        ProjectTreeListParams(workspaceId: workspaceId, path: path, showIgnored: showIgnored))
    ).decoded(ProjectTreeListResult.self)
    // A listing that raced a toggle of ignored files describes the old view.
    guard projectTree(workspaceId).showIgnored == showIgnored else { return }
    var tree = projectTree(workspaceId)
    tree.apply(listing)
    editorWorkspace.trees[workspaceId] = tree
  }
}
