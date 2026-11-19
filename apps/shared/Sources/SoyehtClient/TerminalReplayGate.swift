import Foundation
import SoyehtProtocol

/// One terminal replay at a time on a connection.
///
/// A subscription from the tail makes the engine queue up to `ProtocolLimits.tailBytes` of the
/// pane's output for this connection, and a connection whose queue passes
/// `ProtocolLimits.connectionQueueBytes` is closed as a slow consumer. Subscribing every terminal
/// in a row, without waiting, put every tail in that queue together: with a handful of terminals
/// holding long output the connection closed, was made again, and closed again. Here a subscribe
/// takes its turn, and the next one is sent only after the engine's `stream.replay_done` for the
/// previous one has been read, which is when its bytes have left the engine's queue. So the replays
/// in the queue never exceed one, whatever the number of terminals, and the same holds for a
/// grid of terminals that all mount at once and for the subscribes after a reconnect.
@MainActor final class TerminalReplayGate {
  private var busy = false
  /// Moves when a connection ends, so a turn taken on the old one never frees one on the new.
  private var epoch = 0
  private var turns: [CheckedContinuation<Int?, Never>] = []
  private var waiting: (paneId: String, token: UInt64, resume: CheckedContinuation<Void, Never>)?
  private var nextToken: UInt64 = 0
  /// Replays that ended before the subscriber began to wait for them.
  private var ended: Set<String> = []
  private let timeout: Duration

  init(timeout: Duration = .seconds(ClientPolicy.requestTimeoutSeconds)) {
    self.timeout = timeout
  }

  /// Waits for the turn. `nil` when the connection ended while waiting: the caller stops.
  func enter() async -> Int? {
    guard busy else {
      busy = true
      return epoch
    }
    return await withCheckedContinuation { turns.append($0) }
  }

  func leave(_ turn: Int) {
    guard turn == epoch else { return }
    guard !turns.isEmpty else {
      busy = false
      return
    }
    turns.removeFirst().resume(returning: epoch)
  }

  /// Called before the subscribe request goes out.
  func expectReplay(of paneId: String) { ended.remove(paneId) }

  /// Waits until the replay of `paneId` has been read, or the engine has been silent for the
  /// request timeout, which must not hold the other terminals back for ever. After it, the next
  /// subscribe may queue a second tail (about 4 MiB in all, within the connection's budget).
  /// A holder of a connection that has ended returns at once.
  func awaitReplayEnd(of paneId: String, turn: Int) async {
    guard turn == epoch else { return }
    if ended.remove(paneId) != nil { return }
    if let stale = waiting {
      waiting = nil
      stale.resume.resume()
    }
    nextToken &+= 1
    let token = nextToken
    let timer = Task { [weak self, timeout] in
      do { try await Task.sleep(for: timeout) } catch { return }  // Cancelled: the replay ended.
      self?.stopWaiting(token)
    }
    await withCheckedContinuation { waiting = (paneId, token, $0) }
    timer.cancel()
  }

  func replayEnded(_ paneId: String) {
    guard let current = waiting, current.paneId == paneId else {
      ended.insert(paneId)
      return
    }
    waiting = nil
    current.resume.resume()
  }

  private func stopWaiting(_ token: UInt64) {
    guard let current = waiting, current.token == token else { return }
    waiting = nil
    current.resume.resume()
  }

  /// The connection ended: every subscriber waiting for a turn or a replay stops.
  func reset() {
    epoch += 1
    busy = false
    ended.removeAll()
    let queued = turns
    turns.removeAll()
    for turn in queued { turn.resume(returning: nil) }
    if let current = waiting {
      waiting = nil
      current.resume.resume()
    }
  }
}
