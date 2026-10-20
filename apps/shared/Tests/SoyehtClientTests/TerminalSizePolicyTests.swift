import Foundation
import SoyehtProtocol
import SwiftTerm
import Testing

@testable import SoyehtClient

private let mac = TerminalGrid(cols: 200, rows: 50)
private let screen = TerminalGrid(cols: 90, rows: 30)

@Test func followingTheMacNeverAsksForAResize() {
  // Shown for the first time, after a rotation, after a text size change: only the view's
  // grid moves to the pane's, nothing goes to the engine.
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .followMac, pane: mac, local: screen, announced: false, requestInFlight: false)
      == .adopt(mac))
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .followMac, pane: mac, local: mac, announced: false, requestInFlight: false) == .none)
  #expect(
    TerminalSizePolicy.layoutChanged(mode: .followMac, pane: mac, natural: screen) == .adopt(mac))
}

@Test func followingTheMacWaitsForAPaneItDoesNotKnow() {
  #expect(TerminalSizePolicy.layoutChanged(mode: .followMac, pane: nil, natural: screen) == .none)
  #expect(
    TerminalSizePolicy.layoutChanged(
      mode: .followMac, pane: TerminalGrid(cols: 0, rows: 0), natural: screen) == .none)
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .followMac, pane: TerminalGrid(cols: 0, rows: 0), local: screen, announced: false,
      requestInFlight: false) == .none)
}

@Test func fittingThisScreenAsksOnceThenFollowsLaterChangesOfThePane() {
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .fitThisScreen, pane: mac, local: screen, announced: false, requestInFlight: false)
      == .request(screen))
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .fitThisScreen, pane: screen, local: screen, announced: true, requestInFlight: false)
      == .none)
  // Someone else resized the pane afterwards: show what the pane holds.
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .fitThisScreen, pane: mac, local: screen, announced: true, requestInFlight: false)
      == .adopt(mac))
}

@Test func fittingThisScreenDoesNotFightItsOwnRequestInFlight() {
  #expect(
    TerminalSizePolicy.reconcile(
      mode: .fitThisScreen, pane: mac, local: screen, announced: true, requestInFlight: true)
      == .none)
  #expect(
    TerminalSizePolicy.layoutChanged(mode: .fitThisScreen, pane: mac, natural: screen)
      == .request(screen))
  #expect(
    TerminalSizePolicy.layoutChanged(
      mode: .fitThisScreen, pane: mac, natural: TerminalGrid(cols: 0, rows: 5)) == .none)
}

@MainActor @Test func paneSizeModeIsPerPaneAndStartsByFollowingTheMac() {
  let modes = TerminalSizeModeStore()
  #expect(modes.mode(for: "pane-a") == .followMac)
  _ = modes.choose(.fitThisScreen, for: "pane-a", paneGrid: mac)
  #expect(modes.mode(for: "pane-a") == .fitThisScreen)
  #expect(modes.mode(for: "pane-b") == .followMac)
  _ = modes.choose(.followMac, for: "pane-a", paneGrid: screen)
  #expect(modes.mode(for: "pane-a") == .followMac)
}

@MainActor @Test func leavingAFitAsksForTheSizeThePaneHadBefore() {
  let modes = TerminalSizeModeStore()
  #expect(modes.choose(.fitThisScreen, for: "pane-a", paneGrid: mac) == nil)
  // Choosing it twice must not replace the remembered size with the fitted one.
  #expect(modes.choose(.fitThisScreen, for: "pane-a", paneGrid: screen) == nil)
  #expect(modes.choose(.followMac, for: "pane-a", paneGrid: screen) == mac)
  // Already following: nothing to restore.
  #expect(modes.choose(.followMac, for: "pane-a", paneGrid: screen) == nil)
}

@MainActor @Test func leavingAFitDoesNothingWhenThereIsNothingToRestore() {
  let modes = TerminalSizeModeStore()
  _ = modes.choose(.fitThisScreen, for: "pane-a", paneGrid: mac)
  #expect(modes.choose(.followMac, for: "pane-a", paneGrid: mac) == nil)
  _ = modes.choose(.fitThisScreen, for: "pane-b", paneGrid: nil)
  #expect(modes.choose(.followMac, for: "pane-b", paneGrid: screen) == nil)
}

private func terminalPane(cols: UInt32, rows: UInt32) -> Pane {
  Pane(
    paneId: "pane-a", workspaceId: "workspace-a", name: "shell", cwd: "/", mode: .terminal,
    state: .idle, source: .none, confidence: 1, phrase: "", generation: 1, cols: cols,
    rows: rows, record: false)
}

@MainActor @Test func showingAPaneWhileFollowingTheMacTakesItsSizeAndSendsNothing() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  var resizes: [(String?, Int, Int)] = []
  let host = TerminalHost(
    client: client, paneId: "pane-a", acceptsInput: true, sizeMode: .followMac,
    resizeTerminal: { resizes.append(($0, $1, $2)) })
  let view = TerminalView(frame: .zero)

  host.update(view, pane: terminalPane(cols: 200, rows: 50), connectionRevision: 1)
  #expect(view.getTerminal().cols == 200)
  #expect(view.getTerminal().rows == 50)

  // The Mac resizes the pane later, and the view takes the new size without asking.
  host.update(view, pane: terminalPane(cols: 120, rows: 40), connectionRevision: 1)
  #expect(view.getTerminal().cols == 120)
  #expect(resizes.isEmpty)
}

@MainActor @Test func showingAPaneWhileFittingAsksOnceAndWaitsForTheAnswer() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  var resizes: [(String?, Int, Int)] = []
  let host = TerminalHost(
    client: client, paneId: "pane-a", acceptsInput: true, sizeMode: .fitThisScreen,
    resizeTerminal: { resizes.append(($0, $1, $2)) })
  let view = TerminalView(frame: .zero)
  let local = view.getTerminal().cols

  host.update(view, pane: terminalPane(cols: 200, rows: 50), connectionRevision: 1)
  host.update(view, pane: terminalPane(cols: 200, rows: 50), connectionRevision: 1)

  // One request, and the pane's old size did not pull the view back while it was in flight.
  #expect(resizes.count == 1)
  #expect(resizes.first?.1 == local)
  #expect(view.getTerminal().cols == local)
}

@Test func theClipboardIsTouchedOnlyWhenAllowedActiveAndVisible() {
  #expect(TerminalClipboardPolicy.accepts(allowed: true, appActive: true, visible: true))
  #expect(!TerminalClipboardPolicy.accepts(allowed: false, appActive: true, visible: true))
  #expect(!TerminalClipboardPolicy.accepts(allowed: true, appActive: false, visible: true))
  #expect(!TerminalClipboardPolicy.accepts(allowed: true, appActive: true, visible: false))
}

@MainActor @Test func programsMayNotCopyUntilThePersonAllowsIt() {
  let preferences = MemoryPreferenceStore()
  let setting = TerminalClipboardPreferences(preferences: preferences)
  #expect(!setting.allowsProgramCopy)
  setting.setAllowsProgramCopy(true)
  #expect(TerminalClipboardPreferences(preferences: preferences).allowsProgramCopy)
  setting.setAllowsProgramCopy(false)
  #expect(!TerminalClipboardPreferences(preferences: preferences).allowsProgramCopy)
}

@MainActor @Test func theCopyNoticeShowsThenHides() async {
  let notice = TerminalCopyNotice(duration: .milliseconds(50))
  #expect(!notice.visible)
  notice.show()
  #expect(notice.visible)
  // Wait for the hide itself: a loaded host can delay it far past any fixed time.
  await notice.hiding?.value
  #expect(!notice.visible)
}

@MainActor @Test func aHostFollowingTheMacSendsNoResizeWhenItsViewChangesSize() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  var resizes: [(String?, Int, Int)] = []
  let following = TerminalHost(
    client: client, paneId: "pane-a", acceptsInput: true, sizeMode: .followMac,
    resizeTerminal: { resizes.append(($0, $1, $2)) })
  let fitted = TerminalHost(
    client: client, paneId: "pane-b", acceptsInput: true, sizeMode: .fitThisScreen,
    resizeTerminal: { resizes.append(($0, $1, $2)) })
  let view = TerminalView(frame: .zero)

  following.sizeChanged(source: view, newCols: 90, newRows: 30)
  #expect(resizes.isEmpty)

  fitted.sizeChanged(source: view, newCols: 90, newRows: 30)
  #expect(resizes.map(\.0) == ["pane-b"])
  #expect(resizes.map(\.1) == [90])

  // Choosing "Fit to this screen" turns the following host into a sizing one.
  #expect(following.updateSizeMode(.fitThisScreen))
  #expect(!following.updateSizeMode(.fitThisScreen))
  following.sizeChanged(source: view, newCols: 100, newRows: 40)
  #expect(resizes.map(\.0) == ["pane-b", "pane-a"])
}

@MainActor @Test func aProgramCopiesToTheClipboardButNeverReadsIt() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  var copied: [String] = []
  let host = TerminalHost(
    client: client, acceptsInput: true, writeClipboard: { text, _ in copied.append(text) })
  let view = TerminalView(frame: .zero)

  host.clipboardCopy(source: view, content: Data("ssh-keygen".utf8))
  host.clipboardCopy(source: view, content: Data())
  host.clipboardCopy(source: view, content: Data([0xff, 0xfe]))

  #expect(copied == ["ssh-keygen"])
  #expect(host.clipboardRead(source: view) == nil)
}
