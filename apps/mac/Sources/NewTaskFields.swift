import SoyehtClient
import SwiftUI

struct TaskField: View {
  let title: String
  @Binding var text: String

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      DialogFieldLabel(title: title)
      TextField("", text: $text).font(MacTheme.chrome(MacTheme.TypeScale.label, weight: .semibold))
        .textFieldStyle(.plain).padding(.horizontal, MacTheme.Space.x14).frame(
          height: DesignTokens.Layout.taskFieldHeight
        )
        .neoWell(radius: MacTheme.Radius.control)
    }
  }
}

struct TaskEditor: View {
  let title: String
  @Binding var text: String
  var showsAccessories = false
  var accessoryAction: (() -> Void)? = nil

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      DialogFieldLabel(title: title)
      ZStack(alignment: .bottomLeading) {
        TextEditor(text: $text).font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .scrollContentBackground(.hidden)
          .padding(
            EdgeInsets(
              top: 8, leading: 10, bottom: showsAccessories ? 28 : 8, trailing: 10))
        if showsAccessories { editorAccessories }
      }.frame(height: DesignTokens.Layout.taskEditorHeight).neoWell(radius: MacTheme.Radius.control)
    }
  }

  private var editorAccessories: some View {
    HStack(spacing: MacTheme.Space.x8) {
      if let accessoryAction {
        Button(action: accessoryAction) {
          Image(systemName: "paperclip").font(
            .system(size: MacTheme.TypeScale.label, weight: .medium))
        }.buttonStyle(.plain).help("Edit verification notes")
          .accessibilityLabel("Edit verification notes")
      } else {
        Image(systemName: "paperclip").font(
          .system(size: MacTheme.TypeScale.label, weight: .medium))
      }
      Image(systemName: "at").font(.system(size: MacTheme.TypeScale.label, weight: .medium))
    }
    .foregroundStyle(MacTheme.muted)
    .padding(.leading, MacTheme.Space.x14).padding(.bottom, MacTheme.Space.x10)
    .accessibilityHidden(accessoryAction == nil)
  }
}

struct DialogFieldLabel: View {
  let title: String

  var body: some View {
    Text(title).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold)).tracking(1)
      .foregroundStyle(MacTheme.muted)
  }
}

struct WorkspaceDefaultToggle: View {
  @Binding var isOn: Bool

  var body: some View {
    Button {
      isOn.toggle()
    } label: {
      HStack(spacing: MacTheme.Space.x6) {
        Text("Use workspace default")
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        ZStack {
          Capsule().fill(MacTheme.well).frame(width: 36, height: 20)
          Color.clear.frame(width: 16, height: 16)
            .neoRaised(radius: MacTheme.Radius.pill, elevation: .toggle)
            .offset(x: isOn ? 8 : -8)
        }.frame(width: 36, height: 20)
      }.foregroundStyle(MacTheme.secondary)
    }
    .buttonStyle(.plain).accessibilityValue(isOn ? "On" : "Off")
  }
}

struct DialogOptionLabel: View {
  let title: String
  let icon: String

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(.system(size: MacTheme.TypeScale.control, weight: .medium))
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
      Image(systemName: "chevron.down").font(
        .system(size: MacTheme.TypeScale.small, weight: .medium)
      )
      .foregroundStyle(MacTheme.muted)
    }
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x10).padding(.vertical, MacTheme.Space.x6)
    .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
  }
}
