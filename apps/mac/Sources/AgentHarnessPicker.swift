import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The agent list, model and effort choices shared by the new-agent sheet and the
/// composer's harness switch.
struct AgentHarnessPicker: View {
  @Bindable var client: EngineClient
  @Binding var harness: String
  @Binding var model: String
  @Binding var effort: String
  let catalogue: AgentCatalogue?
  let effortOption: ConfigOption?
  let excludingHarness: String?
  @State private var query = ""
  @State private var showingMore = false

  private var layout: HarnessPickerLayout {
    HarnessPickerModel.layout(
      client.harnesses.filter { $0.id != excludingHarness }, query: query)
  }

  /// "More harnesses" opens on request, while searching, when nothing installed is
  /// left to show, or when the selection lives there.
  private func moreExpanded(_ layout: HarnessPickerLayout) -> Bool {
    showingMore || !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      || layout.sections.isEmpty || layout.more.contains { $0.id == harness }
  }

  private var contentState: HarnessPickerContentState {
    HarnessPickerModel.contentState(
      harnesses: client.harnesses.filter { $0.id != excludingHarness }, query: query,
      loading: client.harnessesLoading, failed: client.loadFailure(.harnesses) != nil)
  }

  private var modelContentState: ModelPickerContentState {
    HarnessPickerModel.modelContentState(
      catalogue: catalogue, loading: client.agentCatalogueLoading(harness),
      failed: client.loadFailure(.catalogue(harness: harness)) != nil)
  }

  private var selected: HarnessInfo? { client.harnesses.first { $0.id == harness } }
  private var selectedModel: ModelInfo? { catalogue?.models.first { $0.id == model } }
  private var excludedQueryMatch: HarnessInfo? {
    guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
      let excludingHarness,
      let excluded = client.harnesses.first(where: { $0.id == excludingHarness }),
      HarnessPickerModel.matches(excluded, query: query)
    else { return nil }
    return excluded
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      HStack(spacing: MacTheme.Space.x8) {
        TextField("Search agents", text: $query)
          .textFieldStyle(.plain)
          .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
          .padding(.horizontal, MacTheme.Space.x10).frame(height: 32)
          .paneWell(radius: MacTheme.Radius.compact)
        Button {
          client.refreshHarnesses(rescan: true)
        } label: {
          Image(systemName: "arrow.clockwise")
        }
        .help("Rescan installed agents")
        .accessibilityLabel("Rescan installed agents")
      }
      if contentState == .loading {
        VStack(spacing: MacTheme.Space.x10) {
          ProgressView()
          Text("Loading agents…")
            .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
        .frame(maxWidth: .infinity, minHeight: 220)
        .accessibilityIdentifier("agent-picker-loading")
      } else if contentState == .failed {
        HarnessLoadStateRow(
          state: .failed(BackgroundLoad.harnesses.failureText), surface: .pane,
          detail: client.loadFailure(.harnesses), retry: { client.refreshHarnesses() }
        )
        .frame(maxWidth: .infinity, minHeight: 220, alignment: .top)
      } else if contentState == .emptyCatalogue {
        ContentUnavailableView(
          "No agents available", systemImage: "cpu",
          description: Text("Rescan after installing or configuring an agent.")
        )
        .frame(maxWidth: .infinity, minHeight: 220)
      } else if contentState == .noMatches {
        if let excludedQueryMatch {
          ContentUnavailableView(
            "\(excludedQueryMatch.name) is already active", systemImage: "checkmark.circle",
            description: Text("Choose another agent to restart this conversation.")
          )
          .frame(maxWidth: .infinity, minHeight: 220)
        } else {
          ContentUnavailableView.search(text: query)
            .frame(maxWidth: .infinity, minHeight: 220)
        }
      } else {
        let shown = layout
        let expanded = moreExpanded(shown)
        ScrollView {
          LazyVStack(alignment: .leading, spacing: MacTheme.Space.x12) {
            ForEach(shown.sections, id: \.group.rawValue) { section in
              Text(section.group.title.uppercased()).pickerHeading()
              ForEach(section.items, id: \.id) { item in harnessRow(item) }
            }
            if !shown.more.isEmpty {
              Button {
                showingMore.toggle()
              } label: {
                HStack(spacing: MacTheme.Space.x6) {
                  Text("\(HarnessPickerLayout.moreTitle.uppercased()) · \(shown.more.count)")
                    .pickerHeading()
                  Image(systemName: expanded ? "chevron.down" : "chevron.right")
                    .font(.system(size: MacTheme.TypeScale.micro, weight: .bold))
                    .foregroundStyle(MacTheme.muted)
                  Spacer()
                }
                .contentShape(Rectangle())
              }
              .buttonStyle(.plain)
              .accessibilityLabel(HarnessPickerLayout.moreTitle)
              .accessibilityValue(expanded ? "Expanded" : "Collapsed")
              .accessibilityIdentifier("agent-choice-more")
              if expanded {
                ForEach(shown.more, id: \.id) { item in harnessRow(item) }
              }
            }
          }
          .padding(.trailing, MacTheme.Space.x4)
        }
        .frame(height: 260)
      }
      Divider().overlay(MacTheme.border)
      selectedConfiguration
    }
    .padding(EdgeInsets(top: 14, leading: 14, bottom: 14, trailing: 14))
    .frame(width: 420, height: 510, alignment: .top)
    .foregroundStyle(MacTheme.ink)
    .background(MacTheme.face)
    .onAppear {
      if selected?.available == true, !harness.isEmpty {
        client.refreshCatalogue(harness: harness)
      }
    }
  }

  private func harnessRow(_ item: HarnessInfo) -> some View {
    let status = HarnessPickerModel.setupState(for: item)
    return Button {
      harness = item.id
    } label: {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: statusSymbol(status))
          .foregroundStyle(statusColor(status)).frame(width: 18)
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text(item.name)
            .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold)).lineLimit(1)
          Text(HarnessPickerModel.statusLabel(for: item))
            .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
        Spacer()
        if item.id == harness { Image(systemName: "checkmark").foregroundStyle(MacTheme.accent) }
      }
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 42)
      .background(
        item.id == harness ? MacTheme.well : .clear,
        in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(item.name), \(HarnessPickerModel.statusLabel(for: item))")
    .accessibilityIdentifier("agent-choice-\(item.id)")
  }

  @ViewBuilder private var selectedConfiguration: some View {
    if let selected, selected.available {
      Text(HarnessPickerModel.statusReason(for: selected))
        .font(MacTheme.ui(MacTheme.TypeScale.small))
        .foregroundStyle(MacTheme.secondary)
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityIdentifier("agent-setup-reason")
      Text("MODEL").pickerHeading()
      if modelContentState == .loading {
        HStack(spacing: MacTheme.Space.x8) {
          ProgressView().controlSize(.small)
          Text("Loading models…")
        }
        .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
        .padding(.horizontal, MacTheme.Space.x10).frame(height: 30)
        .paneWell(radius: MacTheme.Radius.compact)
        .accessibilityIdentifier("model-picker-loading")
      } else {
        if modelContentState == .failed {
          HarnessLoadStateRow(
            state: .failed(BackgroundLoad.catalogue(harness: harness).failureText),
            surface: .pane, detail: client.loadFailure(.catalogue(harness: harness)),
            retry: { client.refreshCatalogue(harness: harness) })
        }
        Menu {
          Button("Agent default") { model = "" }
          ForEach(catalogue?.models ?? [], id: \.id) { item in
            Button {
              model = item.id
            } label: {
              if item.observedRefusal == nil {
                Text(item.name)
              } else {
                Label(
                  HarnessPickerModel.modelLabel(for: item),
                  systemImage: "exclamationmark.triangle.fill")
              }
            }
          }
        } label: {
          HStack {
            Text(selectedModelLabel)
            Spacer()
            Image(systemName: "chevron.up.chevron.down")
              .font(.system(size: MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
          }
          .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
          .padding(.horizontal, MacTheme.Space.x10).frame(height: 30)
          .paneWell(radius: MacTheme.Radius.compact)
        }
        .menuStyle(.borderlessButton)
        if let reason = HarnessPickerModel.modelRefusalReason(for: selectedModel) {
          Label {
            Text("This agent previously refused this model: \(reason)")
          } icon: {
            Image(systemName: "exclamationmark.triangle.fill")
          }
          .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(MacTheme.warning)
          .fixedSize(horizontal: false, vertical: true)
          .accessibilityIdentifier("model-observed-refusal")
        }
        if modelContentState == .empty {
          Text("This agent provided no model choices.")
            .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        } else if modelContentState == .unavailable {
          Text("Models will appear after this agent starts.")
            .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
      }
      if let choices = effortOption?.options, !choices.isEmpty {
        Text("EFFORT").pickerHeading()
        HStack(spacing: MacTheme.Space.x2) {
          ForEach(choices, id: \.value) { choice in
            Button(choice.name) { effort = choice.value }
              .font(
                MacTheme.ui(
                  MacTheme.TypeScale.control, weight: effort == choice.value ? .bold : .semibold)
              )
              .frame(maxWidth: .infinity).padding(.vertical, MacTheme.Space.x6)
              .background(effort == choice.value ? MacTheme.face : .clear, in: Capsule())
          }
        }.padding(3).background(MacTheme.well, in: Capsule())
      }
    } else if let selected {
      Label(
        HarnessPickerModel.statusReason(for: selected),
        systemImage: "exclamationmark.circle"
      )
      .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
      .foregroundStyle(MacTheme.muted)
      .accessibilityIdentifier("agent-setup-reason")
    } else {
      Text("Choose an agent to continue.")
        .font(MacTheme.ui(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
    }
  }

  private var selectedModelLabel: String {
    selectedModel.map { HarnessPickerModel.modelLabel(for: $0) } ?? "Agent default"
  }

  private func statusSymbol(_ status: HarnessSetupState) -> String {
    switch status {
    case .ready: "checkmark.circle.fill"
    case .not_checked: "circle.dotted"
    case .sign_in: "person.crop.circle.badge.exclamationmark"
    case .check_setup: "wrench.and.screwdriver"
    case .not_installed: "minus.circle"
    }
  }

  private func statusColor(_ status: HarnessSetupState) -> Color {
    switch status {
    case .ready: MacTheme.success
    case .not_checked: MacTheme.muted
    case .sign_in, .check_setup: MacTheme.warning
    case .not_installed: MacTheme.muted
    }
  }
}
