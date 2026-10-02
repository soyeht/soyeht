import SoyehtClient
import SwiftUI

/// The in-file find and replace bar (Cmd+F, Cmd+Option+F). The engine
/// searches the whole file; this view only binds fields and forwards
/// commands to the shared controller, so the iPad can reuse the logic.
struct EditorFindBar: View {
  @Bindable var find: EditorFindController
  let revision: String
  @FocusState private var focus: Field?

  private enum Field { case query, replacement }

  var body: some View {
    VStack(spacing: MacTheme.Space.x6) {
      HStack(spacing: MacTheme.Space.x6) {
        iconButton(
          find.showsReplace ? "chevron.down" : "chevron.right", help: "Toggle replace"
        ) { find.showsReplace.toggle() }
        field("Find", text: Binding(get: { find.query }, set: { find.setQuery($0) }), .query)
          .onKeyPress(.return, phases: .down) { press in
            press.modifiers.contains(.shift) ? find.previousMatch() : find.nextMatch()
            return .handled
          }
        toggle("Aa", help: "Match case", \.caseSensitive)
        toggle("ab", help: "Match whole word", \.wholeWord)
        toggle(".*", help: "Use regular expression", \.regex)
        Text(find.status)
          .font(MacTheme.mono(MacTheme.TypeScale.small))
          .foregroundStyle(find.problem == nil ? MacTheme.secondary : MacTheme.danger)
          .lineLimit(1).frame(minWidth: 72, alignment: .trailing)
        iconButton("chevron.up", help: "Previous match (⇧⌘G)", action: find.previousMatch)
          .keyboardShortcut(
            focus == nil ? nil : KeyboardShortcut("g", modifiers: [.command, .shift])
          )
          .disabled(!find.hasMatches)
        iconButton("chevron.down", help: "Next match (⌘G)", action: find.nextMatch)
          .keyboardShortcut(focus == nil ? nil : KeyboardShortcut("g", modifiers: .command))
          .disabled(!find.hasMatches)
        iconButton("xmark", help: "Close (Esc)", action: find.close)
      }
      if find.showsReplace {
        HStack(spacing: MacTheme.Space.x6) {
          field("Replace", text: $find.replacement, .replacement)
            .padding(.leading, PaneLayoutContentMetrics.editorFindIndent)
            .onSubmit(find.replaceCurrent)
          pill("Replace", primary: false, action: find.replaceCurrent)
          pill("Replace All", primary: true, action: find.replaceAll)
        }
      }
    }
    .padding(.horizontal, MacTheme.Space.x10).padding(.vertical, MacTheme.Space.x6)
    .background(MacTheme.canvas)
    .onExitCommand(perform: find.close)
    .onAppear { focus = .query }
    .onChange(of: find.fieldFocusRequest) { focus = .query }
    .onChange(of: revision) { find.refresh() }
  }

  private func field(_ title: String, text: Binding<String>, _ kind: Field) -> some View {
    TextField(title, text: text)
      .textFieldStyle(.plain)
      .font(MacTheme.mono(MacTheme.TypeScale.small))
      .foregroundStyle(MacTheme.ink)
      .focused($focus, equals: kind)
      .padding(.horizontal, MacTheme.Space.x8)
      .frame(height: PaneLayoutContentMetrics.editorFindFieldHeight)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
  }

  private func toggle(
    _ label: String, help: String, _ option: WritableKeyPath<EditorFindOptions, Bool>
  ) -> some View {
    let isOn = find.options[keyPath: option]
    return Button {
      var options = find.options
      options[keyPath: option].toggle()
      find.setOptions(options)
    } label: {
      Text(label)
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(isOn ? MacTheme.accent : MacTheme.secondary)
        .padding(.horizontal, MacTheme.Space.x6)
        .frame(height: PaneLayoutContentMetrics.editorFindFieldHeight)
        .background(isOn ? MacTheme.well : Color.clear, in: Capsule())
    }
    .buttonStyle(.plain).help(help)
    .accessibilityAddTraits(isOn ? .isSelected : [])
  }

  private func iconButton(
    _ symbol: String, help: String, action: @escaping () -> Void
  ) -> some View {
    let side = PaneLayoutContentMetrics.editorFindFieldHeight
    return Button(action: action) {
      Image(systemName: symbol)
        .frame(width: side, height: side)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        .contentShape(Rectangle())
    }
    .buttonStyle(.plain).foregroundStyle(MacTheme.secondary).help(help)
  }

  private func pill(_ title: String, primary: Bool, action: @escaping () -> Void) -> some View {
    Button(action: action) {
      Text(title)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        .foregroundStyle(primary ? MacTheme.face : MacTheme.ink)
        .padding(.horizontal, MacTheme.Space.x12)
        .frame(height: PaneLayoutContentMetrics.editorFindFieldHeight)
        .background(primary ? MacTheme.ink : MacTheme.well, in: Capsule())
    }
    .buttonStyle(.plain).disabled(!find.hasMatches)
  }
}
