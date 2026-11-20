import Foundation
import Observation
import SoyehtProtocol

/// One pane's value, observed on its own: a view that shows one pane follows this box and no
/// other, so a change in another pane does not run it.
@MainActor @Observable
final class PaneBox {
  fileprivate(set) var pane: Pane?

  init(_ pane: Pane? = nil) { self.pane = pane }
}

/// The panes of one workspace, or of one mode, observed as one list: a pane that changes in
/// another workspace does not change it.
@MainActor @Observable
final class PaneSlice {
  fileprivate(set) var panes: [Pane] = []
}

/// The panes of a client, observed at the grain the views read them: the whole list (every change
/// of every pane), one pane (`pane(_:)`), the panes of one workspace or of one mode. A write
/// changes the list and the boxes it concerns, so what runs is what shows the pane that changed.
@MainActor @Observable
final class PaneStore {
  /// The whole list. Kept apart from the property that readers follow, so making a box or a slice
  /// inside a view does not make that view follow every pane.
  @ObservationIgnored private var list: [Pane] = []
  @ObservationIgnored private var boxes: [String: PaneBox] = [:]
  @ObservationIgnored private var byWorkspace: [String: PaneSlice] = [:]
  @ObservationIgnored private var byMode: [PaneMode: PaneSlice] = [:]
  /// Told after every write, for what is made from the whole list (the attention summary).
  @ObservationIgnored var didChange: (@MainActor () -> Void)?

  var all: [Pane] {
    access(keyPath: \.all)
    return list
  }

  /// The list as it is, for what is made from it; reading it follows nothing.
  var allUntracked: [Pane] { list }

  private func setAll(_ panes: [Pane]) {
    withMutation(keyPath: \.all) { list = panes }
    didChange?()
  }

  func pane(_ paneId: String?) -> Pane? {
    guard let paneId else { return nil }
    return box(paneId).pane
  }

  func panes(in workspaceId: String) -> [Pane] {
    slice(workspace: workspaceId).panes
  }

  func panes(mode: PaneMode) -> [Pane] {
    slice(mode: mode).panes
  }

  /// Every pane replaced: all readers are told.
  func replaceAll(_ panes: [Pane]) {
    let before = list
    setAll(panes)
    let ids = Set(panes.map(\.paneId))
    for pane in panes { box(pane.paneId).pane = pane }
    for gone in before where !ids.contains(gone.paneId) { forget(gone.paneId) }
    for (workspaceId, slice) in byWorkspace {
      slice.panes = panes.filter { $0.workspaceId == workspaceId }
    }
    for (mode, slice) in byMode { slice.panes = panes.filter { $0.mode == mode } }
  }

  /// One pane added or replaced in place.
  func upsert(_ pane: Pane) {
    let previous = box(pane.paneId).pane
    var next = list
    if let index = next.firstIndex(where: { $0.paneId == pane.paneId }) {
      next[index] = pane
    } else {
      next.append(pane)
    }
    setAll(next)
    box(pane.paneId).pane = pane
    refreshSlices(for: pane)
    if let previous, previous.workspaceId != pane.workspaceId || previous.mode != pane.mode {
      refreshSlices(for: previous)
    }
  }

  /// One pane changed in place; nothing happens for a pane the client does not hold.
  func update(_ paneId: String, _ change: (inout Pane) -> Void) {
    guard var pane = box(paneId).pane else { return }
    change(&pane)
    upsert(pane)
  }

  func remove(_ paneId: String) {
    guard let removed = box(paneId).pane else { return }
    setAll(list.filter { $0.paneId != paneId })
    forget(paneId)
    refreshSlices(for: removed)
  }

  /// A pane that is gone: its readers are told, and the store keeps nothing of it.
  private func forget(_ paneId: String) {
    boxes[paneId]?.pane = nil
    boxes.removeValue(forKey: paneId)
  }

  private func box(_ paneId: String) -> PaneBox {
    if let box = boxes[paneId] { return box }
    let box = PaneBox(list.first { $0.paneId == paneId })
    boxes[paneId] = box
    return box
  }

  private func slice(workspace workspaceId: String) -> PaneSlice {
    if let slice = byWorkspace[workspaceId] { return slice }
    let slice = PaneSlice()
    slice.panes = list.filter { $0.workspaceId == workspaceId }
    byWorkspace[workspaceId] = slice
    return slice
  }

  private func slice(mode: PaneMode) -> PaneSlice {
    if let slice = byMode[mode] { return slice }
    let slice = PaneSlice()
    slice.panes = list.filter { $0.mode == mode }
    byMode[mode] = slice
    return slice
  }

  private func refreshSlices(for pane: Pane) {
    byWorkspace[pane.workspaceId]?.panes = list.filter { $0.workspaceId == pane.workspaceId }
    byMode[pane.mode]?.panes = list.filter { $0.mode == pane.mode }
  }
}
