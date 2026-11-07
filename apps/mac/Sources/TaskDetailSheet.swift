import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct TaskDetailSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let taskId: String
  let close: () -> Void

  private var detail: TaskShowResult? { client.taskDetails[taskId] }
  private var task: TaskRecord? { detail?.task ?? client.tasks.first { $0.taskId == taskId } }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      detailHeader
      if let task {
        ScrollView { detailContent(task) }.scrollIndicators(.never)
        commentPreview
        actionPreview
      } else {
        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .padding(EdgeInsets(top: 18, leading: 20, bottom: 18, trailing: 20))
    .taskSheetFrame()
    .foregroundStyle(MacTheme.ink).neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
  }

  private var detailHeader: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Text(taskId).font(MacTheme.mono(MacTheme.TypeScale.control, weight: .bold))
        .foregroundStyle(MacTheme.muted)
      Spacer()
      Image(systemName: "arrow.up.left.and.arrow.down.right")
        .font(.system(size: MacTheme.TypeScale.label, weight: .medium)).foregroundStyle(
          MacTheme.muted)
      Button(action: close) {
        Image(systemName: "xmark").font(.system(size: MacTheme.TypeScale.label, weight: .medium))
          .foregroundStyle(MacTheme.muted)
      }.buttonStyle(.plain)
    }
  }

  private func detailContent(_ task: TaskRecord) -> some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text(task.title).font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
        .lineSpacing(4).fixedSize(horizontal: false, vertical: true)
      taskFacts(task)
      runnerFacts(task)
      DetailSection(title: "DESCRIPTION", text: task.description)
      protectedBoundary(task.mustNotChange)
      if let verify = task.howToVerify { DetailSection(title: "HOW TO VERIFY", text: verify) }
      if let blocking = task.blocking {
        DetailSection(
          title: "BLOCKING · \(blocking.kind.title.uppercased())", text: blocking.reason)
      }
      subitems
      attempts
      summary(task)
      driftReview
      timeline
    }.frame(maxWidth: .infinity, alignment: .leading)
  }

  private func taskFacts(_ task: TaskRecord) -> some View {
    HStack(spacing: MacTheme.Space.x8) {
      DetailStatePill(state: task.state)
      TaskPriorityChip(priority: task.priority)
      if let name = client.ownerName(of: task) { TaskOwnerChip(name: name) }
      if let badge = task.reviewBadge { DetailPill(text: badge.title, color: badge.color) }
    }
  }

  private func runnerFacts(_ task: TaskRecord) -> some View {
    HStack(spacing: MacTheme.Space.x8) {
      Label(TaskPresentation.runnerName(task), systemImage: "cpu")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.ink)
        .padding(.horizontal, MacTheme.Space.x10).padding(.vertical, MacTheme.Space.x4)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
      if let branch = task.worktree?.branch {
        Label("\(branch) · attempt \(task.currentAttempt)", systemImage: "arrow.triangle.branch")
          .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
          .foregroundStyle(MacTheme.muted).lineLimit(1)
      }
    }
  }

  private func protectedBoundary(_ text: String) -> some View {
    DetailGroup(title: "MUST NOT CHANGE") {
      Text(text).font(MacTheme.chrome(MacTheme.TypeScale.control))
        .textSelection(.enabled).padding(MacTheme.Space.x10).frame(
          maxWidth: .infinity, alignment: .leading
        )
        .neoWell(radius: MacTheme.Radius.compact)
    }
  }

  private var subitems: some View {
    let values = detail?.subitems ?? []
    return DetailGroup(title: TaskPresentation.subitemsTitle(values)) {
      if detail?.subitems.isEmpty != false {
        Text("No subitems.").font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(values, id: \.subitemId) { item in
          HStack(spacing: MacTheme.Space.x8) {
            Image(systemName: item.state == .done ? "checkmark.circle.fill" : "circle")
              .font(.system(size: MacTheme.TypeScale.label)).foregroundStyle(
                item.state == .done ? MacTheme.success : MacTheme.muted)
            Text(item.text).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .medium))
              .foregroundStyle(item.state == .done ? MacTheme.muted : MacTheme.ink)
            Spacer()
            if let agent = item.agent {
              Text(agent).font(MacTheme.chrome(MacTheme.TypeScale.small))
                .foregroundStyle(MacTheme.muted)
            }
          }
        }
      }
    }
  }

  private var attempts: some View {
    DetailGroup(title: "ATTEMPTS") {
      ForEach(detail?.attempts ?? [], id: \.n) { attempt in
        VStack(alignment: .leading, spacing: 3) {
          Text(TaskPresentation.attemptLine(attempt)).font(
            MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
          Text(TaskPresentation.attemptOutcome(attempt)).font(
            MacTheme.chrome(MacTheme.TypeScale.small)
          ).foregroundStyle(MacTheme.secondary)
          if let value = attempt.summary {
            Text(value).font(MacTheme.chrome(MacTheme.TypeScale.small))
          }
        }
      }
    }
  }

  private func summary(_ task: TaskRecord) -> some View {
    DetailGroup(title: "SUMMARY") {
      Text(task.summary ?? "No summary yet.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(task.summary == nil ? MacTheme.secondary : MacTheme.ink)
    }
  }

  private var driftReview: some View {
    let lines = TaskPresentation.driftFindings(detail?.comments ?? [])
    return DetailGroup(title: "DRIFT REVIEW") {
      DeclarationList(
        title: "Contract changes",
        values: detail?.task.contractChanges?.map(TaskPresentation.contractChange) ?? [])
      DeclarationList(
        title: "Out-of-scope edits",
        values: detail?.task.outOfScopeEdits?.map(TaskPresentation.outOfScopeEdit) ?? [])
      if lines.isEmpty {
        Text("No reviewer findings.").font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
          FilePathText(line, workspaceId: workspaceId)
            .font(MacTheme.mono(MacTheme.TypeScale.small)).textSelection(.enabled)
        }
      }
    }
  }

  private var timeline: some View {
    DetailGroup(title: "HISTORY AND COMMENTS") {
      ForEach(timelineEntries, id: \.id) { entry in
        HStack(alignment: .top, spacing: MacTheme.Space.x8) {
          Image(systemName: entry.author == nil ? "circle.fill" : "person.crop.circle.fill")
            .font(.system(size: MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
            .padding(.top, MacTheme.Space.x3)
          VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
            if let author = entry.author {
              Text(author).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
                .padding(.horizontal, MacTheme.Space.x8).padding(.vertical, MacTheme.Space.x2)
                .background(taskOwnerColor(author), in: Capsule())
            }
            FilePathText(entry.text, workspaceId: workspaceId)
              .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
              .foregroundStyle(entry.author == nil ? MacTheme.secondary : MacTheme.ink)
              .textSelection(.enabled)
            Text(entry.at).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
              .foregroundStyle(MacTheme.muted)
          }.frame(maxWidth: .infinity, alignment: .leading)
        }.padding(.vertical, MacTheme.Space.x3)
      }
    }
  }

  private var timelineEntries: [TaskPresentation.TimelineEntry] {
    detail.map(TaskPresentation.timeline) ?? []
  }

  private var commentPreview: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Text("Comment…").font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.muted)
      Spacer()
      Image(systemName: "arrow.up").font(.system(size: MacTheme.TypeScale.label, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
    }.padding(.horizontal, MacTheme.Space.x14).frame(height: 44).neoWell(
      radius: MacTheme.Radius.control
    )
    .accessibilityHidden(true)
  }

  private var actionPreview: some View {
    HStack(spacing: MacTheme.Space.x10) {
      PreviewAction(title: "Dispatch to pane", icon: "paperplane", trailing: true)
      PreviewAction(title: "Send for review", icon: "eye", trailing: false)
      Spacer()
      Image(systemName: "ellipsis").font(
        .system(size: DesignTokens.Glyph.taskSheetMore, weight: .medium)
      )
      .foregroundStyle(MacTheme.muted)
    }.accessibilityHidden(true)
  }
}

struct DetailStatePill: View {
  let state: TaskState

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Text(state.title).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
      Image(systemName: "chevron.down").font(.system(size: MacTheme.TypeScale.small, weight: .bold))
    }
    .foregroundStyle(MacTheme.onAccent)
    .padding(.horizontal, MacTheme.Space.x10).padding(.vertical, MacTheme.Space.x4)
    .background(state.color, in: Capsule())
  }
}

struct PreviewAction: View {
  let title: String
  let icon: String
  let trailing: Bool

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Image(systemName: icon).font(.system(size: MacTheme.TypeScale.caption, weight: .semibold))
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      if trailing {
        Image(systemName: "chevron.down").font(
          .system(size: MacTheme.TypeScale.small, weight: .semibold)
        )
        .foregroundStyle(MacTheme.muted)
      }
    }
    .foregroundStyle(MacTheme.ink)
    .padding(.horizontal, MacTheme.Space.x14).padding(.vertical, MacTheme.Space.x8)
    .neoRaised(radius: MacTheme.Radius.pill, elevation: .card)
  }
}

struct DetailPill: View {
  let text: String
  let color: Color
  var body: some View {
    Text(text).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .padding(.horizontal, MacTheme.Space.x9).padding(.vertical, MacTheme.Space.x5)
      .background(color.opacity(0.14), in: Capsule()).foregroundStyle(color)
  }
}

struct DetailSection: View {
  let title: String
  let text: String
  var body: some View {
    DetailGroup(title: title) {
      Text(text).font(MacTheme.chrome(MacTheme.TypeScale.control)).textSelection(.enabled)
    }
  }
}

struct DetailGroup<Content: View>: View {
  let title: String
  @ViewBuilder let content: Content
  var body: some View {
    VStack(alignment: .leading, spacing: 9) {
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold)).tracking(1.1)
        .foregroundStyle(MacTheme.secondary)
      content
    }.frame(maxWidth: .infinity, alignment: .leading)
  }
}

struct DeclarationList: View {
  let title: String
  let values: [String]
  var body: some View {
    VStack(alignment: .leading, spacing: 5) {
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
      if values.isEmpty {
        Text("None declared.").font(MacTheme.chrome(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(values, id: \.self) { Text($0).font(MacTheme.mono(MacTheme.TypeScale.small)) }
      }
    }.padding(MacTheme.Space.x12).neoWell(radius: MacTheme.Radius.compact)
  }
}
