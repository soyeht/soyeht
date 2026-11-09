import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

private enum PhoneWritingSplitChoice: String, CaseIterable, Identifiable {
  case text
  case screenplay
  case presentation
  case agent

  var id: String { rawValue }
  var title: String {
    switch self {
    case .text: "Text"
    case .screenplay: "Screenplay"
    case .presentation: "Presentation"
    case .agent: "Agent"
    }
  }
  var kind: PaneLayoutKind {
    switch self {
    case .text: .text
    case .screenplay: .screenplay
    case .presentation: .presentation
    case .agent: .agent
    }
  }
  var section: WritingSection? {
    switch self {
    case .text: .texts
    case .screenplay: .screenplays
    case .presentation: .presentations
    case .agent: nil
    }
  }
}

private struct PhoneWritingSplitRequest: Identifiable {
  let id = UUID()
  let anchorPaneId: String
  let anchorKind: PaneLayoutKind
  let axis: PaneLayoutAxis
  let baseTree: PaneLayoutTree
}

@MainActor
struct PhoneWritingWorkspace: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let initialPaneId: String

  @State private var localState: PhoneWritingLocalState?
  @State private var showingNewText = false
  @State private var newTextTitle = ""
  @State private var creatingText = false
  @State private var splitRequest: PhoneWritingSplitRequest?
  @State private var splitChoice: PhoneWritingSplitChoice = .text
  @State private var splitTitle = ""
  @State private var splitHarnessId = ""
  @State private var creatingSplit = false
  @State private var scrollActivityTick = 0
  private let stateStore: PhoneWritingLocalStateStore
  private let engineId: String

  init(
    client: EngineClient, workspaceId: String, initialPaneId: String,
    stateStore: PhoneWritingLocalStateStore = PhoneWritingLocalStateStore()
  ) {
    self.client = client
    self.workspaceId = workspaceId
    self.initialPaneId = initialPaneId
    self.stateStore = stateStore
    engineId = client.engineId ?? "unpaired"
    _localState = State(
      initialValue: stateStore.load(
        engineId: client.engineId ?? "unpaired", workspaceId: workspaceId))
  }

  var body: some View {
    GeometryReader { proxy in
      let plan = layoutPlan(viewport: proxy.size)
      Group {
        if plan.leaves.isEmpty {
          ContentUnavailableView(
            "No writing documents", systemImage: "text.book.closed",
            description: Text("Create a text document or open one on the Mac."))
        } else {
          ScrollView(.vertical) {
            ZStack(alignment: .topLeading) {
              Color.clear
                .frame(width: plan.contentSize.width, height: plan.contentSize.height)
                .allowsHitTesting(false)
              ForEach(plan.leaves, id: \.identity) { leaf in
                PhoneWritingLeaf(
                  client: client, leaf: leaf,
                  pane: pane(withId: leaf.identity.paneId),
                  scrollActivityTick: scrollActivityTick,
                  onAction: { action in apply(action, to: plan) },
                  onSplit: { axis in beginSplit(leaf: leaf, axis: axis) }
                )
                .writingFrame(leaf.rowRect)
              }
            }
          }
          .scrollBounceBehavior(.basedOnSize)
          .simultaneousGesture(
            DragGesture(minimumDistance: 3).onChanged { _ in
              scrollActivityTick &+= 1
            })
        }
      }
      .background(PhonePalette.canvas)
      .onAppear {
        reconcile(plan.localState)
      }
      .onChange(of: plan.localState) { _, reconciled in reconcile(reconciled) }
    }
    .navigationTitle(workspaceName)
    .navigationBarTitleDisplayMode(.inline)
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          newTextTitle = ""
          showingNewText = true
        } label: {
          Image(systemName: "plus")
        }
        .accessibilityLabel("New text document")
        .accessibilityIdentifier("phone-writing-new-text")
      }
    }
    .sheet(isPresented: $showingNewText) { newTextSheet }
    .sheet(item: $splitRequest) { request in splitSheet(request) }
  }

  private var newTextSheet: some View {
    NavigationStack {
      Form {
        TextField("Title", text: $newTextTitle)
          .textInputAutocapitalization(.sentences)
          .accessibilityIdentifier("phone-writing-new-text-title")
      }
      .navigationTitle("New text")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { showingNewText = false }
        }
        ToolbarItem(placement: .confirmationAction) {
          Button("Create") { createText() }
            .disabled(
              newTextTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                || creatingText
            )
            .accessibilityIdentifier("phone-writing-create-text")
        }
      }
    }
    .presentationDetents([.medium])
  }

  private func splitSheet(_ request: PhoneWritingSplitRequest) -> some View {
    NavigationStack {
      Form {
        Picker("Pane", selection: $splitChoice) {
          ForEach(availableSplitChoices(for: request)) { choice in
            Text(choice.title).tag(choice)
          }
        }
        .phoneWritingSheetRow()
        if splitChoice == .agent {
          Picker("Harness", selection: $splitHarnessId) {
            ForEach(availableHarnesses, id: \.id) { harness in
              Text(harness.name).tag(harness.id)
            }
          }
          .phoneWritingSheetRow()
          if availableHarnesses.isEmpty {
            Text("No ready agent harness is available on this Mac.")
              .foregroundStyle(PhonePalette.secondary)
              .phoneWritingSheetRow()
          }
        } else {
          TextField(
            "", text: $splitTitle,
            prompt: Text("Title").foregroundStyle(PhonePalette.secondary)
          )
          .textInputAutocapitalization(.sentences)
          .accessibilityIdentifier("phone-writing-split-title")
          .phoneWritingSheetRow()
        }
      }
      .phoneWritingSheetScroll()
      .phoneWritingSheetNavigation(
        request.axis == .horizontal ? "Split columns" : "Split rows"
      )
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { splitRequest = nil }
        }
        ToolbarItem(placement: .confirmationAction) {
          Button("Create") { createSplit(request) }
            .disabled(!splitIsReady || creatingSplit)
            .accessibilityIdentifier("phone-writing-create-split")
        }
      }
    }
    .phoneWritingSheet(.split)
    .onChange(of: availableHarnesses.map(\.id)) { _, harnessIds in
      guard splitChoice == .agent, !harnessIds.contains(splitHarnessId) else { return }
      splitHarnessId =
        HarnessPickerModel.defaultHarness(
          in: availableHarnesses, preferredId: client.preferredHarnessId)?.id ?? ""
    }
  }

  private var availableHarnesses: [HarnessInfo] {
    HarnessPickerModel.sections(client.harnesses)
      .flatMap(\.items)
      .filter(\.available)
  }

  private func availableSplitChoices(
    for request: PhoneWritingSplitRequest
  ) -> [PhoneWritingSplitChoice] {
    let allowed = PhoneWritingMetrics.chromePolicy.plan(kind: request.anchorKind).splitKinds
    return PhoneWritingSplitChoice.allCases.filter { allowed.contains($0.kind) }
  }

  private var splitIsReady: Bool {
    if splitChoice == .agent { return availableHarnesses.contains { $0.id == splitHarnessId } }
    return !splitTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  private var workspaceName: String {
    client.workspaces.first { $0.workspaceId == workspaceId }?.name ?? "Writing"
  }

  private func pane(withId paneId: String) -> Pane? {
    client.panes.first { $0.paneId == paneId }
  }

  private func layoutPlan(viewport: CGSize) -> PhoneWritingLayoutPlan {
    let panes = client.panes.filter { $0.workspaceId == workspaceId }
    let paneNames = Dictionary(uniqueKeysWithValues: panes.map { ($0.paneId, $0.name) })
    let documents = client.writingDocuments.values.compactMap {
      view -> PhoneWritingDocumentDescriptor? in
      let document = view.document
      guard document.file.workspaceId == workspaceId,
        let pane = panes.first(where: { $0.paneId == document.file.paneId })
      else { return nil }
      let kind = PaneLayoutKind(mode: pane.mode, writingKind: document.kind)
      return PhoneWritingDocumentDescriptor(
        paneId: pane.paneId, fileId: document.file.fileId, name: pane.name, kind: kind,
        font: PhoneWritingMetrics.metric(for: document), fullWidth: document.fullWidth)
    }
    return PhoneWritingMetrics.policy.layout(
      PhoneWritingLayoutRequest(
        tree: layoutTree(livePanes: panes), documents: documents, paneNames: paneNames,
        // The pane used to enter this phone screen seeds device-local state without
        // mutating the engine's shared focus.
        selectedPaneId: initialPaneId, localState: localState,
        snapshotIsAuthoritative: client.connected,
        viewport: PaneLayoutSize(width: viewport.width, height: viewport.height)))
  }

  private func layoutTree(livePanes: [Pane]) -> PaneLayoutTree? {
    client.presentedLayoutTree(
      workspaceId: workspaceId, livePanes: livePanes, sizing: PhoneWritingMetrics.sizing)
  }

  private func apply(_ action: PhoneWritingStateAction, to plan: PhoneWritingLayoutPlan) {
    let updated = PhoneWritingMetrics.policy.applying(action, to: plan)
    localState = updated
    persist(updated)
  }

  private func reconcile(_ state: PhoneWritingLocalState) {
    // A paired-device disconnect clears replicas by design. Do not mistake that transient
    // empty snapshot for panes being closed and overwrite the last local presentation.
    guard client.connected else { return }
    guard localState != state else { return }
    localState = state
    persist(state)
  }

  private func persist(_ state: PhoneWritingLocalState) {
    do {
      try stateStore.save(state, engineId: engineId, workspaceId: workspaceId)
    } catch {
      client.problem = "Could not remember the iPhone writing view."
    }
  }

  private func beginSplit(leaf: PhoneWritingLeafLayout, axis: PaneLayoutAxis) {
    let panes = client.panes.filter { $0.workspaceId == workspaceId }
    guard let tree = layoutTree(livePanes: panes) else {
      client.problem = "Could not open the split chooser. The layout was not changed."
      return
    }
    splitChoice = .text
    splitTitle = ""
    client.refreshHarnesses()
    splitHarnessId =
      HarnessPickerModel.defaultHarness(
        in: client.harnesses, preferredId: client.preferredHarnessId)?.id ?? ""
    splitRequest = PhoneWritingSplitRequest(
      anchorPaneId: leaf.identity.paneId, anchorKind: leaf.kind, axis: axis, baseTree: tree)
  }

  private func createSplit(_ request: PhoneWritingSplitRequest) {
    guard splitIsReady, !creatingSplit else { return }
    let choice = splitChoice
    let title = splitTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    let harnessId = splitHarnessId
    creatingSplit = true
    Task { @MainActor in
      defer { creatingSplit = false }
      do {
        let insertion = PaneLayoutProvisionalInsertion(
          baseTree: request.baseTree,
          placeholder: PaneLayoutLeaf(paneId: "phone-new-pane", kind: choice.kind),
          placement: .split(anchorPaneId: request.anchorPaneId, axis: request.axis))
        let paneId: String
        let completed: PaneLayoutTree
        if let section = choice.section {
          let result = try await client.createWritingDocument(
            workspaceId: workspaceId, section: section, title: title,
            selectPane: false)
          paneId = result.pane.paneId
          completed = try insertion.completedTree(
            with: PaneLayoutLeaf(paneId: paneId, kind: choice.kind),
            sizing: PhoneWritingMetrics.sizing)
          let identity = PhoneWritingIdentity(
            paneId: paneId, fileId: result.document.file.fileId)
          let state = (localState ?? PhoneWritingLocalState()).opening(identity)
          localState = state
          persist(state)
        } else {
          guard let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
          else { throw PhoneWritingSplitFailure.missingWorkspace }
          paneId = try await client.createAgentPaneNow(
            directory: URL(fileURLWithPath: workspace.path), harness: harnessId,
            model: nil, effort: nil, selectPane: false)
          completed = try insertion.completedWritingCompanionTree(
            with: PaneLayoutLeaf(paneId: paneId, kind: .agent),
            writingPaneId: request.anchorPaneId, sizing: PhoneWritingMetrics.sizing)
        }
        try client.paneLayouts.commit(tree: completed, workspaceId: workspaceId)
        splitRequest = nil
      } catch {
        client.problem = "Could not create the split. The layout was not changed."
      }
    }
  }

  private func createText() {
    let title = newTextTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !title.isEmpty, !creatingText else { return }
    creatingText = true
    Task { @MainActor in
      defer { creatingText = false }
      do {
        let result = try await client.createWritingDocument(
          workspaceId: workspaceId, section: .texts, title: title, selectPane: false)
        let identity = PhoneWritingIdentity(
          paneId: result.pane.paneId, fileId: result.document.file.fileId)
        let state = (localState ?? PhoneWritingLocalState()).opening(identity)
        localState = state
        persist(state)
        showingNewText = false
      } catch {
        client.problem = "Could not create the text document."
      }
    }
  }
}

private enum PhoneWritingSplitFailure: Error {
  case missingWorkspace
}
