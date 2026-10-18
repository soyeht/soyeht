import Foundation
import SoyehtClient
import SoyehtProtocol
import SwiftUI

private let beatCategories: [BeatCategory] = [.story, .character, .setting, .research]

struct WritingBeatBoardPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let document: WritingDocument
  let structure: WritingStructureResult
  let showScreenplay: () -> Void
  @State private var beats: [BeatCard] = []
  @State private var editor: BeatDraft?
  @State private var loading = false

  private var fileId: String { document.file.fileId }
  private var title: String {
    URL(fileURLWithPath: document.file.path).deletingPathExtension().lastPathComponent
  }

  var body: some View {
    VStack(spacing: 0) {
      header
      board
    }
    .task(id: "\(fileId):\(document.sidecarRevision)") { await reload() }
    .sheet(item: $editor) { draft in
      BeatEditorSheet(client: client, fileId: fileId, scenes: structure.scenes, draft: draft) {
        editor = nil
        Task { await reload() }
      }
    }
    .accessibilityIdentifier("writing-beat-board")
  }

  private var header: some View {
    GeometryReader { geometry in
      let plan = WritingBeatBoardHeaderPolicy().plan(viewportWidth: geometry.size.width)
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: "lightbulb")
          .foregroundStyle(MacTheme.secondary)
        Text("Idea board — \(title)")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .lineLimit(1)
          .truncationMode(.middle)
        Spacer(minLength: MacTheme.Space.x4)
        Button {
          showScreenplay()
        } label: {
          headerActionLabel("Screenplay", icon: "doc.text", plan: plan)
        }
        .buttonStyle(.plain)
        .foregroundStyle(MacTheme.secondary)
        .accessibilityLabel("Screenplay")
        Button {
          editor = .new(order: UInt32(clamping: beats.count))
        } label: {
          headerActionLabel("Idea", icon: "plus", plan: plan)
        }
        .buttonStyle(NeoPillButtonStyle())
        .accessibilityLabel("New idea")
        .accessibilityIdentifier("writing-new-beat")
      }
      .padding(.horizontal, MacTheme.Space.x14)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    .frame(height: 54)
    .background(MacTheme.well, in: Capsule())
    .padding(.horizontal, MacTheme.Space.x10).padding(.top, MacTheme.Space.x10)
    .padding(.bottom, MacTheme.Space.x6)
  }

  @ViewBuilder private func headerActionLabel(
    _ title: String, icon: String, plan: WritingBeatBoardHeaderPlan
  ) -> some View {
    if plan.showsActionTitles {
      Label(title, systemImage: icon)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
        .lineLimit(1)
        .fixedSize(horizontal: true, vertical: false)
    } else {
      Image(systemName: icon)
        .font(.system(size: MacTheme.TypeScale.control, weight: .bold))
        .frame(width: 18, height: 18)
        .accessibilityHidden(true)
    }
  }

  private var board: some View {
    ZStack(alignment: .bottom) {
      ScrollView {
        LazyVGrid(
          columns: [GridItem(.adaptive(minimum: 220, maximum: 286), spacing: 28)],
          alignment: .leading, spacing: 40
        ) {
          ForEach(beats, id: \.beatId) { beat in
            beatCard(beat)
          }
          newIdeaCard
        }
        .padding(.horizontal, 34).padding(.top, 30).padding(.bottom, 76)
      }
      Text("Drag an idea over another card to reorganize it. Link it to a scene in its details.")
        .font(MacTheme.chrome(MacTheme.TypeScale.control))
        .foregroundStyle(MacTheme.muted)
        .padding(.horizontal, MacTheme.Space.x14).frame(height: 34)
        .background(MacTheme.canvas.opacity(0.92), in: Capsule())
        .padding(.bottom, MacTheme.Space.x12)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(MacTheme.well)
    .overlay {
      if loading && beats.isEmpty { ProgressView() }
    }
  }

  private func beatCard(_ beat: BeatCard) -> some View {
    Button {
      editor = BeatDraft(beat)
    } label: {
      VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
        HStack(spacing: MacTheme.Space.x7) {
          Circle().fill(beat.category.tint).frame(width: 8, height: 8)
          Text(beat.category.label.uppercased())
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
            .foregroundStyle(MacTheme.secondary).tracking(0.7)
          Spacer()
        }
        Text(beat.title)
          .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          .foregroundStyle(MacTheme.ink).lineLimit(2)
        Text(beat.body.isEmpty ? "Add a note to this idea." : beat.body)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(beat.body.isEmpty ? MacTheme.muted : MacTheme.secondary)
          .lineLimit(4).frame(maxWidth: .infinity, alignment: .leading)
        Spacer(minLength: 0)
        scenePill(beat.sceneId)
      }
      .padding(MacTheme.Space.x14)
      .frame(maxWidth: .infinity, minHeight: 172, alignment: .topLeading)
      .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
    }
    .buttonStyle(.plain)
    .draggable(beat.beatId)
    .dropDestination(for: String.self) { ids, _ in reorder(ids.first, before: beat) }
    .contextMenu {
      Button("Edit idea") { editor = BeatDraft(beat) }
      Button("Delete idea", role: .destructive) { delete(beat) }
    }
    .accessibilityIdentifier("writing-beat-\(beat.beatId)")
  }

  private func scenePill(_ sceneId: String?) -> some View {
    let scene = structure.scenes.first { $0.sceneId == sceneId }
    return Label(
      scene.map { "Scene \($0.ordinal + 1)" } ?? "No scene",
      systemImage: scene == nil ? "link.badge.plus" : "arrow.turn.down.right"
    )
    .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
    .foregroundStyle(scene == nil ? MacTheme.muted : MacTheme.accent)
    .padding(.horizontal, MacTheme.Space.x8).frame(height: 24)
    .background(scene == nil ? MacTheme.well : MacTheme.selection, in: Capsule())
  }

  private var newIdeaCard: some View {
    Button {
      editor = .new(order: UInt32(clamping: beats.count))
    } label: {
      VStack(spacing: MacTheme.Space.x8) {
        Image(systemName: "plus").font(.system(size: 20, weight: .light))
        Text("New idea").font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
      }
      .foregroundStyle(MacTheme.muted)
      .frame(maxWidth: .infinity, minHeight: 172)
      .contentShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
    .buttonStyle(.plain)
  }

  private func reload() async {
    loading = true
    defer { loading = false }
    do { beats = try await client.listWritingBeats(fileId: fileId).beats } catch {
      client.problem = error.localizedDescription
    }
  }

  private func reorder(_ movingId: String?, before target: BeatCard) -> Bool {
    guard let movingId, movingId != target.beatId,
      let moving = beats.first(where: { $0.beatId == movingId })
    else { return false }
    Task { @MainActor in
      do {
        _ = try await client.putWritingBeat(
          fileId: fileId, beatId: moving.beatId, title: moving.title, body: moving.body,
          category: moving.category, order: target.order, sceneId: moving.sceneId)
        await reload()
      } catch { client.problem = error.localizedDescription }
    }
    return true
  }

  private func delete(_ beat: BeatCard) {
    Task { @MainActor in
      do {
        _ = try await client.deleteWritingBeat(fileId: fileId, beatId: beat.beatId)
        await reload()
      } catch { client.problem = error.localizedDescription }
    }
  }
}

private struct BeatDraft: Identifiable {
  let id = UUID()
  let beatId: String?
  let order: UInt32
  let title: String
  let body: String
  let category: BeatCategory
  let sceneId: String?

  init(_ beat: BeatCard) {
    beatId = beat.beatId
    order = beat.order
    title = beat.title
    body = beat.body
    category = beat.category
    sceneId = beat.sceneId
  }

  static func new(order: UInt32) -> BeatDraft {
    BeatDraft(order: order)
  }

  private init(order: UInt32) {
    beatId = nil
    self.order = order
    title = ""
    body = ""
    category = .story
    sceneId = nil
  }
}

private struct BeatEditorSheet: View {
  @Bindable var client: EngineClient
  let fileId: String
  let scenes: [ScreenplayScene]
  let draft: BeatDraft
  let complete: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var title: String
  @State private var bodyText: String
  @State private var category: BeatCategory
  @State private var sceneId: String?
  @State private var saving = false

  init(
    client: EngineClient, fileId: String, scenes: [ScreenplayScene], draft: BeatDraft,
    complete: @escaping () -> Void
  ) {
    self.client = client
    self.fileId = fileId
    self.scenes = scenes
    self.draft = draft
    self.complete = complete
    _title = State(initialValue: draft.title)
    _bodyText = State(initialValue: draft.body)
    _category = State(initialValue: draft.category)
    _sceneId = State(initialValue: draft.sceneId)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      Text(draft.beatId == nil ? "New idea" : "Edit idea")
        .font(MacTheme.chrome(MacTheme.TypeScale.title, weight: .bold))
      TextField("Idea title", text: $title)
        .textFieldStyle(.plain).font(MacTheme.chrome(MacTheme.TypeScale.body, weight: .bold))
        .padding(MacTheme.Space.x10).neoWell(radius: MacTheme.Radius.control)
      TextEditor(text: $bodyText)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .scrollContentBackground(.hidden).padding(MacTheme.Space.x8)
        .frame(height: 112).neoWell(radius: MacTheme.Radius.control)
      ViewThatFits(in: .horizontal) {
        HStack(spacing: MacTheme.Space.x10) {
          categoryPicker
          scenePicker
        }
        VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
          categoryPicker
          scenePicker
        }
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.control))
      HStack {
        Button("Cancel") { dismiss() }.buttonStyle(.paneSecondary)
        Spacer()
        Button("Save idea") { save() }
          .buttonStyle(.panePrimary).disabled(!valid || saving)
      }
    }
    .padding(MacTheme.Space.x18).macSheetSurface(.ideaEditor).background(MacTheme.canvas)
  }

  private var categoryPicker: some View {
    Picker("Category", selection: $category) {
      ForEach(beatCategories, id: \.self) { value in Text(value.label).tag(value) }
    }
  }

  private var scenePicker: some View {
    Picker("Scene", selection: $sceneId) {
      Text("No scene").tag(String?.none)
      ForEach(scenes, id: \.sceneId) { scene in
        Text("Scene \(identifier: scene.ordinal + 1) · \(scene.heading)")
          .tag(String?.some(scene.sceneId))
      }
    }
  }

  private var valid: Bool {
    !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  private func save() {
    saving = true
    Task { @MainActor in
      do {
        _ = try await client.putWritingBeat(
          fileId: fileId, beatId: draft.beatId,
          title: title.trimmingCharacters(in: .whitespacesAndNewlines),
          body: bodyText.trimmingCharacters(in: .whitespacesAndNewlines),
          category: category, order: draft.order, sceneId: sceneId)
        dismiss()
        complete()
      } catch {
        saving = false
        client.problem = error.localizedDescription
      }
    }
  }
}

@MainActor extension BeatCategory {
  fileprivate var label: String {
    switch self {
    case .story: "Beat"
    case .character: "Character"
    case .setting: "Setting"
    case .research: "Question"
    }
  }

  fileprivate var tint: Color {
    switch self {
    case .story: MacTheme.accent
    case .character: MacTheme.success
    case .setting: MacTheme.warning
    case .research: MacTheme.warning
    }
  }
}
