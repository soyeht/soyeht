import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/pane-layout.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("pane-layout") }

private func tree(_ value: JsonValue) throws -> PaneLayoutTree {
  if let paneId = value["paneId"].string {
    let kind = try #require(value["kind"].string.flatMap(PaneLayoutKind.init(rawValue:)))
    return .leaf(PaneLayoutLeaf(paneId: paneId, kind: kind))
  }
  let axis = try #require(value["axis"].string.flatMap(PaneLayoutAxis.init(rawValue:)))
  let children = try (value["children"].array ?? []).map {
    PaneLayoutBranch(weight: try #require($0["weight"].double), tree: try tree($0["node"]))
  }
  let companion = value["companion"]
  return .split(
    PaneLayoutSplit(
      axis: axis, children: children,
      companion: companion["writingPaneId"].string.map {
        PaneLayoutCompanion(
          writingPaneId: $0, agentPaneId: companion["agentPaneId"].string ?? "")
      }))
}

private func optionalTree(_ value: JsonValue) throws -> PaneLayoutTree? {
  value == .null ? nil : try tree(value)
}

private func pane(_ entry: JsonValue) throws -> Pane {
  var fields: [String: JsonValue] = [
    "paneId": entry["paneId"], "workspaceId": .string("w"), "name": entry["paneId"],
    "cwd": .string("/work"), "mode": entry["mode"], "state": .string("idle"),
    "source": .string("none"), "confidence": .number(0), "phrase": .string(""),
    "generation": .integer(1), "cols": .integer(80), "rows": .integer(24), "record": .bool(false),
  ]
  if entry["writingKind"] != .null { fields["writingKind"] = entry["writingKind"] }
  return try JsonValue.object(fields).decoded(Pane.self)
}

/// The file's floor for every kind, except the kinds a case lists in `floors`.
private func sizing(_ all: JsonValue, floors listed: JsonValue = .null) throws
  -> PaneLayoutSizingPolicy
{
  let floor = PaneLayoutSize(
    width: try #require(all["floor"]["width"].double),
    height: try #require(all["floor"]["height"].double))
  let gap = try #require(all["gap"].double)
  var floors = Dictionary(uniqueKeysWithValues: PaneLayoutKind.allCases.map { ($0, floor) })
  if case .object(let sizes) = listed {
    for (name, size) in sizes {
      let kind = try #require(PaneLayoutKind(rawValue: name))
      floors[kind] = PaneLayoutSize(
        width: try #require(size.array?[0].double), height: try #require(size.array?[1].double))
    }
  }
  return PaneLayoutSizingPolicy(
    floors: floors, gaps: PaneLayoutGaps(horizontal: gap, vertical: gap))
}

@Test func normalizingMergesAndCollapsesAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "normalize") {
    let name = item["name"].string ?? ""
    #expect(try tree(item["tree"]).normalized() == tree(item["expected"]), "\(name)")
  }
}

@Test func splittingPlacesTheNewPaneAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "split") {
    let name = item["name"].string ?? ""
    let newPane = item["newPane"]
    let leaf = PaneLayoutLeaf(
      paneId: newPane["paneId"].string ?? "",
      kind: PaneLayoutKind(
        mode: try JsonValue.string(newPane["mode"].string ?? "").decoded(PaneMode.self)))
    let axis = try #require(item["axis"].string.flatMap(PaneLayoutAxis.init(rawValue:)))
    let placed = try? PaneLayoutTreeMutation.splitting(
      try tree(item["tree"]), anchorPaneId: item["anchor"].string ?? "", newLeaf: leaf,
      axis: axis)
    #expect(placed == (try optionalTree(item["expected"])), "\(name)")
  }
}

@Test func aStoredArrangementIsReadAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "stored") {
    let name = item["name"].string ?? ""
    let panes = try (item["live"].array ?? []).map(pane)
    let state = PaneLayoutState(workspaceLayout: item["layout"], livePanes: panes)
    #expect(state.tree == (try optionalTree(item["expected"])), "\(name)")
  }
}

@Test func theWidthATreeNeedsIsTheVectorMinimum() throws {
  let all = try vector()
  let policy = try sizing(all)
  for item in ContractVector.cases(all, "minimumWidth") {
    let size = try PaneLayoutMinimumSize.require(
      for: tree(item["tree"]), floors: policy.floors, gaps: policy.gaps)
    #expect(size.width == item["minimumWidth"].double, "\(item["name"].string ?? "")")
  }
}

@Test func panesSitWhereTheVectorSaysAtTheMinimumWidth() throws {
  let all = try vector()
  let policy = try sizing(all)
  for item in ContractVector.cases(all, "paneSpans") {
    let name = item["name"].string ?? ""
    let node = try tree(item["tree"])
    let minimum = try PaneLayoutMinimumSize.require(
      for: node, floors: policy.floors, gaps: policy.gaps)
    let rects = try PaneLayoutGeometry.layout(
      tree: node, viewport: PaneLayoutSize(width: minimum.width, height: 5000), sizing: policy
    ).paneRects
    guard case .object(let spans) = item["spans"] else { continue }
    for (id, span) in spans {
      let rect = try #require(rects[id], "\(name): \(id)")
      #expect(rect.origin.x == span.array?[0].double, "\(name): \(id)")
      #expect(rect.size.width == span.array?[1].double, "\(name): \(id)")
    }
    #expect(rects["gone"] == nil)
  }
}

@Test func panesTheTreeDoesNotListAreArrangedAsTheVectorSays() throws {
  let all = try vector()
  #expect(all["companionStrip"].double == PaneLayoutContentMetrics.companionTabStripHeight)
  for item in ContractVector.cases(all, "arrange") {
    let name = item["name"].string ?? ""
    let panes = try (item["live"].array ?? []).map(pane)
    let policy = try sizing(all, floors: item["floors"])
    var state = PaneLayoutState(workspaceLayout: item["layout"], livePanes: panes)
    try state.reconcile(livePanes: panes, sizing: policy)
    #expect(state.tree == (try optionalTree(item["expected"])), "\(name)")
  }
}
