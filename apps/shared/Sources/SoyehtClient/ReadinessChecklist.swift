import Foundation
import SoyehtProtocol

/// What the setup screen lists: the engine's answer to `engine.readiness` plus the one row only
/// the client can answer, whether it reaches the engine at all. The desktop client's
/// `soyeht_client::readiness` follows the same rule; `contracts/vectors/readiness.json` checks
/// both.
public struct ReadinessChecklist: Sendable, Equatable {
  public enum Kind: String, Sendable, Decodable {
    case engine
    case tool
    case harness
  }

  public enum State: String, Sendable, Decodable {
    case ready
    case missing
  }

  public struct Row: Sendable, Equatable, Identifiable, Decodable {
    public let id: String
    public let title: String
    public let kind: Kind
    public let required: Bool
    public let state: State
    /// The command that installs it on this machine, while it is missing.
    public let hint: String?
    /// What the command needs first (Homebrew), while it is missing.
    public let note: String?
  }

  public let rows: [Row]
  /// Everything the app needs is here: the engine, every required tool and an agent.
  public let complete: Bool
  /// The required rows that are missing.
  public let blockers: Int
  public let headline: String

  public func rows(of kind: Kind) -> [Row] { rows.filter { $0.kind == kind } }

  /// The checklist for a client that is (`connected`) or is not connected to the engine, and the
  /// engine's latest answer, if one arrived.
  public static func make(connected: Bool, result: EngineReadinessResult?) -> ReadinessChecklist {
    let answered = connected ? result : nil
    let engine = Row(
      id: "engine", title: "Soyeht engine", kind: .engine, required: true,
      state: connected ? .ready : .missing, hint: nil, note: nil)
    let rows = [engine] + (answered?.items.map(row) ?? [])
    let blockers = rows.filter { $0.required && $0.state == .missing }.count
    let hasAgent = rows.contains { $0.kind == .harness && $0.state == .ready }
    let headline =
      if !connected {
        "The Soyeht engine is not running"
      } else if answered == nil {
        "Checking this machine"
      } else if blockers > 0 {
        "\(blockers) required \(blockers == 1 ? "item" : "items") missing"
      } else if !hasAgent {
        "Install an agent to start"
      } else {
        "Everything is ready"
      }
    return ReadinessChecklist(
      rows: rows, complete: connected && answered != nil && blockers == 0 && hasAgent,
      blockers: blockers, headline: headline)
  }

  private static func row(_ item: ReadinessItem) -> Row {
    Row(
      id: item.id, title: item.name, kind: item.kind == .tool ? .tool : .harness,
      required: item.required, state: item.found ? .ready : .missing,
      hint: item.found ? nil : item.hint, note: item.found ? nil : item.note)
  }
}

extension EngineClient {
  /// The setup checklist for what the engine last said about this machine.
  public var readinessChecklist: ReadinessChecklist {
    ReadinessChecklist.make(connected: connected, result: readiness)
  }

  public func refreshReadiness() {
    performBackground(.readiness) {
      self.readiness = try await self.request(.engineReadiness, .encoded(EngineReadinessParams()))
        .decoded(EngineReadinessResult.self)
    }
  }
}
