import Foundation
import SoyehtProtocol

/// A way to sign in, as the sign-in card and the account menu list it.
public struct AgentSignInMethod: Equatable, Identifiable, Sendable {
  public let id: String
  public let name: String
  public let detail: String?
  /// The method runs in a terminal pane next to the agent.
  public let opensTerminal: Bool

  init(_ method: AuthMethod) {
    id = method.id
    name = method.name
    detail = method.description
    opensTerminal = method.terminal
  }
}

/// What the sign-in card of an agent pane shows. The engine owns the sign-in; this only
/// names what it reports, so every client says the same thing.
public enum AgentSignInState: Equatable, Sendable {
  /// Nothing to sign in to and nothing running.
  case hidden
  /// The agent needs a sign-in. `failure` says why the last attempt did not finish.
  case choose(methods: [AgentSignInMethod], failure: String?)
  /// A sign-in runs in a terminal pane; the person finishes it in the browser.
  case waiting(terminalPaneId: String?)
  /// The sign-in ended and the agent restarts to check the new account.
  case verifying
  case signingOut
  /// A sign-in did not finish on a pane that keeps its previous account.
  case failed(reason: String)

  public init(agent: AgentSession?, authMethods: [AuthMethod], failure: String?) {
    switch agent?.login?.phase {
    case .signing_out: self = .signingOut
    case .verifying: self = .verifying
    case .waiting: self = .waiting(terminalPaneId: agent?.login?.terminalPaneId)
    case nil:
      if !authMethods.isEmpty {
        self = .choose(methods: authMethods.map(AgentSignInMethod.init), failure: failure)
      } else if let failure {
        self = .failed(reason: failure)
      } else {
        self = .hidden
      }
    }
  }

  public var title: String? {
    switch self {
    case .hidden: nil
    case .choose: "Sign in to continue"
    case .waiting: "Finish signing in"
    case .verifying: "Checking the new account"
    case .signingOut: "Signing out"
    case .failed: "Sign-in did not finish"
    }
  }

  public var detail: String? {
    switch self {
    case .hidden, .choose: nil
    case .waiting:
      "Complete the sign-in in your browser. Soyeht continues as soon as it is done."
    case .verifying: "Restarting the agent. The conversation stays."
    case .signingOut: "The agent is forgetting its account."
    case .failed(let reason): reason
    }
  }

  /// The state is moving on its own, so the card shows progress rather than choices.
  public var isWorking: Bool {
    switch self {
    case .verifying, .signingOut: true
    case .hidden, .choose, .waiting, .failed: false
    }
  }
}

/// What a signed-in agent pane offers about its account.
public struct AgentAccountActions: Equatable, Sendable {
  public static let sectionTitle = "Account"
  public static let switchTitle = "Switch account"
  public static let signOutTitle = "Sign out"
  public static let explanation =
    "Switching signs in again in a terminal pane. The conversation and its history stay."

  /// One entry per way to sign in again.
  public let switchMethods: [AgentSignInMethod]
  public let canSignOut: Bool
  /// The engine refuses an account change while a turn runs or another change is in flight.
  public let isEnabled: Bool

  /// `nil` when the agent offers nothing, for example while it still needs its first sign-in.
  public init?(agent: AgentSession?, paneState: PaneState?, connected: Bool) {
    let methods = (agent?.reauthMethods ?? []).map(AgentSignInMethod.init)
    let canSignOut = agent?.canSignOut == true
    guard !methods.isEmpty || canSignOut else { return nil }
    switchMethods = methods
    self.canSignOut = canSignOut
    let turnRunning = [.working, .waiting_for_you, .cancelling, .no_signal].contains(paneState)
    isEnabled = connected && agent?.operation == nil && !turnRunning
  }

  /// The one list of account actions and their labels. Every client renders these items in
  /// its own container (the Mac popover, the iPhone and iPad menu) and runs them the same way.
  public var items: [AgentAccountItem] {
    let switches = switchMethods.map { method in
      AgentAccountItem(
        action: .switchTo(methodId: method.id),
        title: switchMethods.count > 1
          ? "\(Self.switchTitle): \(method.name)" : Self.switchTitle,
        systemImage: "person.crop.circle.badge.checkmark",
        accessibilityIdentifier: "agent-switch-account-\(method.id)")
    }
    let signOut = AgentAccountItem(
      action: .signOut, title: Self.signOutTitle,
      systemImage: "rectangle.portrait.and.arrow.right",
      accessibilityIdentifier: "agent-sign-out")
    return canSignOut ? switches + [signOut] : switches
  }
}

/// One thing a person can do about the account of a signed-in agent.
public struct AgentAccountItem: Equatable, Identifiable, Sendable {
  public enum Action: Equatable, Sendable {
    case switchTo(methodId: String)
    case signOut
  }

  public let action: Action
  public let title: String
  public let systemImage: String
  public let accessibilityIdentifier: String

  public var id: String { accessibilityIdentifier }
  public var isDestructive: Bool { action == .signOut }
}

extension EngineClient {
  /// Starts a sign-in. A pane that is already signed in switches to the account the person
  /// chooses; the engine restarts its session afterwards and keeps the conversation.
  public func startAgentAuthentication(_ paneId: String, methodId: String?) {
    agentPanes[paneId, default: AgentPaneView()].signInFailure = nil
    perform {
      let result = try await self.mutation(
        .paneAuthStart,
        PaneAuthStartParams(
          paneId: paneId, authMethodId: methodId, idempotencyKey: UUID().uuidString)
      ).decoded(PaneAuthStartResult.self)
      if let terminal = result.terminalPaneId { self.select(terminal) }
    }
  }

  /// Hides the reason the last sign-in did not finish.
  public func dismissSignInFailure(_ paneId: String) {
    agentPanes[paneId]?.signInFailure = nil
  }

  /// Stops the running sign-in and closes its terminal.
  public func cancelAgentAuthentication(_ paneId: String) {
    perform {
      _ = try await self.mutation(
        .paneAuthCancel,
        PaneAuthCancelParams(paneId: paneId, idempotencyKey: UUID().uuidString))
    }
  }

  /// Runs an account item the same way on every client.
  public func run(_ item: AgentAccountItem, paneId: String) {
    switch item.action {
    case .switchTo(let methodId): startAgentAuthentication(paneId, methodId: methodId)
    case .signOut: signOutAgent(paneId)
    }
  }

  /// Asks the agent to forget its account; it then asks for a sign-in again.
  public func signOutAgent(_ paneId: String) {
    perform {
      _ = try await self.mutation(
        .paneAuthLogout,
        PaneAuthLogoutParams(paneId: paneId, idempotencyKey: UUID().uuidString))
    }
  }
}
