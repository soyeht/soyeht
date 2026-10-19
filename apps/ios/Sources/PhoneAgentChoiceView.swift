import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The agent, model and effort of a new agent pane. The lists follow the same rules as the
/// Mac sheet (`HarnessPickerModel`); the model and the longer lists open as screens of their
/// own so that nothing runs off a small display.
struct PhoneAgentChoiceView: View {
  let client: EngineClient
  @Binding var form: NewPaneForm
  @State private var query = ""
  @State private var showingMore = false

  private var harnesses: [HarnessInfo] { client.harnesses }
  private var layout: HarnessPickerLayout { HarnessPickerModel.layout(harnesses, query: query) }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[form.harness] }
  private var selected: HarnessInfo? { client.harness(id: form.harness) }

  var body: some View {
    List {
      harnessSections
      if let selected, selected.available {
        configuration(for: selected)
      } else if let selected {
        unavailable(selected)
      }
    }
    .scrollContentBackground(.hidden)
    .background(PhonePalette.canvas)
    .navigationTitle("Agent")
    .navigationBarTitleDisplayMode(.inline)
    .searchable(text: $query, prompt: "Search agents")
    .toolbar {
      ToolbarItem(placement: .primaryAction) {
        Button {
          client.refreshHarnesses(rescan: true)
        } label: {
          Label("Check agents again", systemImage: "arrow.clockwise")
        }
      }
    }
    .task(id: form.harness) {
      if selected?.available == true { client.refreshCatalogue(harness: form.harness) }
    }
  }

  @ViewBuilder private var harnessSections: some View {
    let state = HarnessPickerModel.contentState(
      harnesses: harnesses, query: query, loading: client.harnessesLoading,
      failed: client.loadFailure(.harnesses) != nil)
    switch state {
    case .loading:
      Section {
        HStack(spacing: DesignTokens.Space.x10) {
          ProgressView()
          Text("Loading agents…").foregroundStyle(PhonePalette.secondary)
        }
      }
      .listRowBackground(PhonePalette.face)
    case .failed:
      Section {
        Text(BackgroundLoad.harnesses.failureText).foregroundStyle(PhonePalette.secondary)
        Button("Try again") { client.refreshHarnesses() }
      }
      .listRowBackground(PhonePalette.face)
    case .emptyCatalogue:
      Section {
        ContentUnavailableView(
          "No agents available", systemImage: "cpu",
          description: Text("Check again after installing or configuring an agent on the Mac."))
      }
      .listRowBackground(PhonePalette.face)
    case .noMatches:
      Section { ContentUnavailableView.search(text: query) }
        .listRowBackground(PhonePalette.face)
    case .results:
      let shown = layout
      ForEach(shown.sections, id: \.group.rawValue) { section in
        Section(section.group.title) {
          ForEach(section.items, id: \.id, content: harnessRow)
        }
        .listRowBackground(PhonePalette.face)
      }
      if !shown.more.isEmpty { moreSection(shown) }
    }
  }

  private func moreSection(_ shown: HarnessPickerLayout) -> some View {
    let expanded = HarnessPickerModel.moreExpanded(
      requested: showingMore, query: query, layout: shown, selectedId: form.harness)
    return Section {
      Button {
        showingMore.toggle()
      } label: {
        HStack {
          Text("\(HarnessPickerLayout.moreTitle) · \(shown.more.count)")
          Spacer()
          Image(systemName: expanded ? "chevron.down" : "chevron.right")
            .foregroundStyle(PhonePalette.muted)
        }
      }
      .buttonStyle(.plain)
      .accessibilityValue(expanded ? "Expanded" : "Collapsed")
      if expanded { ForEach(shown.more, id: \.id, content: harnessRow) }
    }
    .listRowBackground(PhonePalette.face)
  }

  private func harnessRow(_ item: HarnessInfo) -> some View {
    let state = HarnessPickerModel.setupState(for: item)
    return Button {
      form.chooseHarness(item.id)
    } label: {
      HStack(spacing: DesignTokens.Space.x10) {
        Image(systemName: HarnessPickerModel.statusSymbol(state))
          .foregroundStyle(statusColor(state))
        VStack(alignment: .leading, spacing: DesignTokens.Space.x2) {
          Text(item.name).font(.headline).lineLimit(1)
          Text(HarnessPickerModel.statusLabel(for: item))
            .font(.caption).foregroundStyle(PhonePalette.secondary)
        }
        Spacer(minLength: 0)
        if item.id == form.harness {
          Image(systemName: "checkmark").foregroundStyle(PhonePalette.accent)
        }
      }
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(item.name), \(HarnessPickerModel.statusLabel(for: item))")
    .accessibilityIdentifier("agent-choice-\(item.id)")
  }

  private func statusColor(_ state: HarnessSetupState) -> Color {
    switch state {
    case .ready: PhonePalette.success
    case .sign_in, .check_setup: PhonePalette.warning
    case .not_checked, .not_installed: PhonePalette.muted
    }
  }

  private func unavailable(_ item: HarnessInfo) -> some View {
    Section {
      Label(HarnessPickerModel.statusReason(for: item), systemImage: "exclamationmark.circle")
        .foregroundStyle(PhonePalette.secondary)
        .accessibilityIdentifier("agent-setup-reason")
    }
    .listRowBackground(PhonePalette.face)
  }

  @ViewBuilder private func configuration(for item: HarnessInfo) -> some View {
    if let notice = HarnessPickerModel.pickerNotice(for: item) {
      Section { Text(notice).foregroundStyle(PhonePalette.secondary) }
        .listRowBackground(PhonePalette.face)
    }
    Section("Model") { modelRows }
      .listRowBackground(PhonePalette.face)
    if let choices = client.effortOption(harness: form.harness)?.options, !choices.isEmpty {
      Section("Effort") {
        // A few levels fit in a row; more open as a list of their own.
        if choices.count < 4 {
          effortPicker(choices).pickerStyle(.segmented)
        } else {
          effortPicker(choices).pickerStyle(.navigationLink)
        }
      }
      .listRowBackground(PhonePalette.face)
    }
  }

  private func effortPicker(_ choices: [ConfigChoice]) -> some View {
    Picker("Effort", selection: $form.effort) {
      Text("Default").tag("")
      ForEach(choices, id: \.value) { Text($0.name).tag($0.value) }
    }
  }

  @ViewBuilder private var modelRows: some View {
    switch HarnessPickerModel.modelContentState(
      catalogue: catalogue, loading: client.agentCatalogueLoading(form.harness),
      failed: client.loadFailure(.catalogue(harness: form.harness)) != nil)
    {
    case .loading:
      HStack(spacing: DesignTokens.Space.x10) {
        ProgressView()
        Text("Loading models…").foregroundStyle(PhonePalette.secondary)
      }
    case .failed:
      Text(BackgroundLoad.catalogue(harness: form.harness).failureText)
        .foregroundStyle(PhonePalette.secondary)
      Button("Try again") { client.refreshCatalogue(harness: form.harness) }
    case .unavailable:
      Text("Models will appear after this agent starts.").foregroundStyle(PhonePalette.secondary)
    case .empty:
      Text("This agent provided no model choices.").foregroundStyle(PhonePalette.secondary)
    case .results:
      Picker("Model", selection: $form.model) {
        Text("Agent default").tag("")
        ForEach(catalogue?.models ?? [], id: \.id) { item in
          Text(HarnessPickerModel.modelLabel(for: item)).tag(item.id)
        }
      }
      .pickerStyle(.navigationLink)
      if let reason = HarnessPickerModel.modelRefusalReason(
        for: catalogue?.models.first { $0.id == form.model })
      {
        Label(
          "This agent previously refused this model: \(reason)",
          systemImage: "exclamationmark.triangle.fill"
        )
        .font(.footnote).foregroundStyle(PhonePalette.warning)
      }
    }
  }
}
