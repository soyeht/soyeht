#if os(iOS)
  import SwiftUI
  import SwiftTerm
  import UIKit

  /// A paired device drives the terminal like the Mac does. SwiftTerm turns the software
  /// keyboard and hardware keys (Return, Backspace, Tab, arrows, Ctrl combinations, Esc,
  /// function keys) into terminal bytes, `PhoneTerminalAccessory` adds the Esc/Ctrl/Tab/arrow
  /// key row, and `TerminalHost` sends them with the same `pane.input` path as the Mac terminal.
  ///
  /// The text size comes from `TerminalFontSizeStore` (pinch, Command-plus and Command-minus,
  /// a menu), and `sizeMode` says whether the pane keeps the Mac's size or is fitted to this
  /// screen (`TerminalSizePolicy`).
  @MainActor public struct PhoneTerminal: UIViewRepresentable {
    let client: EngineClient
    let paneId: String?
    let claimsKeyboardFocus: Bool
    let fontStore: TerminalFontSizeStore
    let sizeMode: TerminalSizeMode
    let onActivate: (() -> Void)?

    /// With `claimsKeyboardFocus` the terminal becomes first responder as soon as it is on
    /// screen, so a hardware keyboard types into it without a tap first. Without a `paneId` the
    /// terminal shows whichever pane the client has selected, alone on the screen; with one it is
    /// one of several on a pane board, and `onActivate` tells that it took the keyboard.
    public init(
      client: EngineClient, paneId: String? = nil, claimsKeyboardFocus: Bool = false,
      fontStore: TerminalFontSizeStore = .shared, sizeMode: TerminalSizeMode = .followMac,
      onActivate: (() -> Void)? = nil
    ) {
      self.client = client
      self.paneId = paneId
      self.claimsKeyboardFocus = claimsKeyboardFocus
      self.fontStore = fontStore
      self.sizeMode = sizeMode
      self.onActivate = onActivate
    }
    public func makeCoordinator() -> TerminalHost {
      let links = PhoneTerminalLinks(client: client)
      return TerminalHost(
        client: client, paneId: paneId, acceptsInput: true,
        openWeb: { workspaceId, url in links.open(url, workspaceId: workspaceId) },
        sizeMode: sizeMode,
        writeClipboard: { text, source in
          let accepted = TerminalClipboardPolicy.accepts(
            allowed: TerminalClipboardPreferences.shared.allowsProgramCopy,
            appActive: UIApplication.shared.applicationState == .active,
            visible: source.window != nil && !source.isHidden)
          guard accepted else { return }
          UIPasteboard.general.string = text
          TerminalCopyNotice.shared.show()
        })
    }
    public func makeUIView(context: Context) -> PhoneTerminalSurface {
      let view = PhoneTerminalSurface(frame: .zero, font: Self.font(fontStore.size))
      view.onActivate = onActivate
      view.claimsKeyboardFocus = claimsKeyboardFocus
      view.linkReporting = .implicit
      view.linkHighlightMode = .hover
      applyTheme(to: view)
      view.accessibilityLabel = "Terminal"
      view.appliedFontSize = fontStore.size
      let store = fontStore
      view.onZoomCommand = { store.apply($0) }
      view.onPinchedSize = { store.set($0, persisting: false) }
      view.onPinchEnded = { [weak host = context.coordinator] in
        store.commit()
        host?.redraw()
      }
      context.coordinator.attach(view)
      return view
    }
    public func updateUIView(_ view: PhoneTerminalSurface, context: Context) {
      view.onActivate = onActivate
      view.claimsKeyboardFocus = claimsKeyboardFocus
      applyTheme(to: view)
      if context.coordinator.updateSizeMode(sizeMode) {
        // The view's own measure is the screen's grid: let it report it again.
        view.measureAgain()
        context.coordinator.redraw()
      }
      if view.applyFontSize(fontStore.size), !view.isPinching { context.coordinator.redraw() }
      context.coordinator.update(view)
    }

    static func font(_ size: Double) -> UIFont {
      .monospacedSystemFont(ofSize: CGFloat(size), weight: .regular)
    }

    private func applyTheme(to view: TerminalView) {
      let colors = SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette)
      view.nativeBackgroundColor = UIColor(colors.face)
      view.nativeForegroundColor = UIColor(colors.ink)
      (view.inputAccessoryView as? PhoneTerminalAccessory)?.apply(
        PhoneTerminalAccessoryColors(colors))
      view.setNeedsDisplay()
    }
    public static func dismantleUIView(_ view: PhoneTerminalSurface, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }

  public final class PhoneTerminalSurface: TerminalView {
    var onActivate: (() -> Void)?
    var claimsKeyboardFocus = false {
      didSet {
        if claimsKeyboardFocus && !oldValue { claimFocus() }
      }
    }
    var appliedFontSize = TerminalFontSizePolicy.defaultSize
    var onZoomCommand: ((TerminalZoomCommand) -> Void)?
    var onPinchedSize: ((Double) -> Void)?
    var onPinchEnded: (() -> Void)?
    private(set) var isPinching = false
    private var pinchBaseSize = TerminalFontSizePolicy.defaultSize
    private var wheel = TerminalWheelAccumulator()

    public override init(frame: CGRect, font: UIFont?) {
      super.init(frame: frame, font: font)
      installPinch()
      installScrollInput()
      installKeyRow()
    }

    public required init?(coder: NSCoder) {
      super.init(coder: coder)
      installPinch()
      installScrollInput()
      installKeyRow()
    }

    /// The row above the keyboard replaces SwiftTerm's, whose keys are 20 points wide.
    private func installKeyRow() {
      let colors = PhoneTerminalAccessoryColors(
        SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette))
      inputAssistantItem.leadingBarButtonGroups = []
      inputAssistantItem.trailingBarButtonGroups = []
      inputAccessoryView = PhoneTerminalAccessory(terminal: self, colors: colors)
    }

    public override func didMoveToWindow() {
      super.didMoveToWindow()
      if claimsKeyboardFocus { claimFocus() }
    }

    /// A tap that gives this terminal the keyboard also gives it the focus.
    public override func becomeFirstResponder() -> Bool {
      let became = super.becomeFirstResponder()
      if became { onActivate?() }
      return became
    }

    private func claimFocus() {
      guard window != nil else { return }
      // SwiftUI inserts the view before its transition settles; claim focus on the next turn.
      DispatchQueue.main.async { [weak self] in
        guard let self, self.window != nil, !self.isFirstResponder else { return }
        _ = self.becomeFirstResponder()
      }
    }

    /// Returns whether the size changed.
    func applyFontSize(_ size: Double) -> Bool {
      guard size != appliedFontSize else { return false }
      appliedFontSize = size
      font = PhoneTerminal.font(size)
      return true
    }

    /// Setting the font makes SwiftTerm measure the view again and report the grid it holds.
    func measureAgain() { font = PhoneTerminal.font(appliedFontSize) }

    // MARK: Zoom

    private func installPinch() {
      addGestureRecognizer(UIPinchGestureRecognizer(target: self, action: #selector(pinched(_:))))
    }

    @objc private func pinched(_ gesture: UIPinchGestureRecognizer) {
      switch gesture.state {
      case .began:
        isPinching = true
        pinchBaseSize = appliedFontSize
      case .changed:
        onPinchedSize?(TerminalFontSizePolicy.pinched(from: pinchBaseSize, scale: gesture.scale))
      case .ended, .cancelled, .failed:
        isPinching = false
        onPinchEnded?()
      default:
        break
      }
    }

    public override var keyCommands: [UIKeyCommand]? {
      let zoom = TerminalZoomShortcut.all.map { shortcut in
        let command = UIKeyCommand(
          title: Self.title(for: shortcut.command), action: #selector(zoomFromKey(_:)),
          input: shortcut.input, modifierFlags: shortcut.shift ? [.command, .shift] : .command)
        command.wantsPriorityOverSystemBehavior = true
        return command
      }
      // Command-C and Command-V reach the terminal even where the app menu has no Edit menu.
      let edit = [("c", #selector(copy(_:))), ("v", #selector(paste(_:)))].map {
        let command = UIKeyCommand(input: $0.0, modifierFlags: .command, action: $0.1)
        command.wantsPriorityOverSystemBehavior = true
        return command
      }
      return (super.keyCommands ?? []) + zoom + edit
    }

    private static func title(for command: TerminalZoomCommand) -> String {
      switch command {
      case .larger: "Larger Text"
      case .smaller: "Smaller Text"
      case .reset: "Default Text Size"
      }
    }

    @objc private func zoomFromKey(_ key: UIKeyCommand) {
      guard let input = key.input,
        let shortcut = TerminalZoomShortcut.matching(
          input: input, shift: key.modifierFlags.contains(.shift))
      else { return }
      onZoomCommand?(shortcut.command)
    }

    // MARK: Trackpad and mouse wheel

    /// A scroll from a trackpad or wheel moves the scrollback on its own, since the terminal
    /// is a scroll view. This reads the same scroll to feed a program that wants the wheel.
    private func installScrollInput() {
      let scroll = UIPanGestureRecognizer(target: self, action: #selector(scrolled(_:)))
      scroll.allowedScrollTypesMask = [.continuous, .discrete]
      scroll.allowedTouchTypes = []
      scroll.cancelsTouchesInView = false
      addGestureRecognizer(scroll)
    }

    @objc private func scrolled(_ gesture: UIPanGestureRecognizer) {
      let terminal = getTerminal()
      let target = TerminalWheelTarget(
        mouseReporting: terminal.mouseMode != .off,
        alternateScreen: terminal.isCurrentBufferAlternate && terminal.alternateScrollMode)
      guard target != .scrollback else {
        wheel.reset()
        return
      }
      if gesture.state == .began { wheel.reset() }
      guard gesture.state == .changed else { return }
      let moved = gesture.translation(in: nil).y
      gesture.setTranslation(.zero, in: nil)
      let lines = wheel.lines(delta: Double(moved), lineHeight: Double(font.lineHeight))
      guard lines != 0 else { return }
      switch target {
      case .mouseWheel: sendWheel(lines: lines, at: gesture.location(in: self))
      case .arrowKeys: sendArrows(lines: lines)
      case .scrollback: break
      }
    }

    private func sendWheel(lines: Int, at point: CGPoint) {
      let terminal = getTerminal()
      let cellWidth = contentSize.width / CGFloat(max(terminal.cols, 1))
      let column = cellWidth > 0 ? Int(point.x / cellWidth) : 0
      let row = Int((point.y - contentOffset.y) / max(font.lineHeight, 1))
      let button = terminal.encodeButton(
        button: lines > 0 ? 4 : 5, release: false, shift: false, meta: false, control: false)
      for _ in 0..<abs(lines) {
        terminal.sendEvent(
          buttonFlags: button, x: max(0, min(terminal.cols - 1, column)),
          y: max(0, min(terminal.rows - 1, row)))
      }
    }

    private func sendArrows(lines: Int) {
      let application = getTerminal().applicationCursor
      let key =
        lines > 0
        ? (application ? EscapeSequences.moveUpApp : EscapeSequences.moveUpNormal)
        : (application ? EscapeSequences.moveDownApp : EscapeSequences.moveDownNormal)
      for _ in 0..<abs(lines) { send(data: key[...]) }
    }
  }
#endif
