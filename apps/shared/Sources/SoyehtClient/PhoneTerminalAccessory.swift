#if os(iOS)
  import Combine
  import SwiftTerm
  import SwiftUI
  import UIKit

  /// The colors of the keys, from the active theme.
  struct PhoneTerminalAccessoryColors {
    let bar: UIColor
    let key: UIColor
    let ink: UIColor
    let selected: UIColor
    let onSelected: UIColor

    init(_ colors: SoyehtThemeColorSet) {
      bar = UIColor(colors.face)
      key = UIColor(colors.well)
      ink = UIColor(colors.ink)
      selected = UIColor(colors.accent)
      onSelected = UIColor(colors.onAccent)
    }
  }

  /// The row of keys above the phone's software keyboard: Esc, Ctrl, Tab, the arrows, the
  /// symbols a phone keyboard buries and the function keys. Every key is `touchTarget` points
  /// square to press; the key drawn is inset inside it so the row stays compact.
  @MainActor final class PhoneTerminalAccessory: UIInputView, UIInputViewAudioFeedback {
    private weak var terminal: TerminalView?
    private var colors: PhoneTerminalAccessoryColors
    private var buttons: [String: UIButton] = [:]
    private var repeatDelay: DispatchWorkItem?
    private var repeatTimer: Timer?
    private var controlReset: AnyCancellable?

    private static let repeatAfter: TimeInterval = 0.6
    private static let repeatEvery: TimeInterval = 0.1

    init(terminal: TerminalView, colors: PhoneTerminalAccessoryColors) {
      self.terminal = terminal
      self.colors = colors
      super.init(
        frame: CGRect(x: 0, y: 0, width: 0, height: PaneInteriorMetrics.touchTarget),
        inputViewStyle: .keyboard)
      allowsSelfSizing = true
      build()
      // SwiftTerm clears Ctrl after the next key; the key follows it.
      controlReset = NotificationCenter.default
        .publisher(for: .terminalViewControlModifierReset, object: terminal)
        .receive(on: DispatchQueue.main)
        .sink { [weak self] _ in self?.buttons["control"]?.isSelected = false }
    }

    required init?(coder: NSCoder) { nil }

    var enableInputClicksWhenVisible: Bool { true }

    override var intrinsicContentSize: CGSize {
      CGSize(width: UIView.noIntrinsicMetric, height: PaneInteriorMetrics.touchTarget)
    }

    func apply(_ colors: PhoneTerminalAccessoryColors) {
      self.colors = colors
      backgroundColor = colors.bar
      for button in buttons.values { button.setNeedsUpdateConfiguration() }
    }

    // MARK: Layout

    private func build() {
      backgroundColor = colors.bar
      let scroll = UIScrollView()
      scroll.showsHorizontalScrollIndicator = false
      scroll.translatesAutoresizingMaskIntoConstraints = false
      let row = UIStackView(arrangedSubviews: PhoneTerminalKey.scrolling.map(makeButton))
      row.translatesAutoresizingMaskIntoConstraints = false
      scroll.addSubview(row)
      let dismiss = makeButton(PhoneTerminalKey.dismiss)
      dismiss.translatesAutoresizingMaskIntoConstraints = false
      addSubview(scroll)
      addSubview(dismiss)
      NSLayoutConstraint.activate([
        scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
        scroll.topAnchor.constraint(equalTo: topAnchor),
        scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
        scroll.trailingAnchor.constraint(equalTo: dismiss.leadingAnchor),
        dismiss.trailingAnchor.constraint(equalTo: trailingAnchor),
        dismiss.topAnchor.constraint(equalTo: topAnchor),
        dismiss.bottomAnchor.constraint(equalTo: bottomAnchor),
        row.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
        row.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
        row.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
        row.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
        row.heightAnchor.constraint(equalTo: scroll.frameLayoutGuide.heightAnchor),
      ])
    }

    private func makeButton(_ key: PhoneTerminalKey) -> UIButton {
      let button = UIButton(type: .custom)
      button.accessibilityLabel = key.label
      button.accessibilityIdentifier = "terminal-key-\(key.id)"
      button.widthAnchor.constraint(equalToConstant: key.pressSize.width).isActive = true
      button.heightAnchor.constraint(equalToConstant: key.pressSize.height).isActive = true
      button.configurationUpdateHandler = { [weak self, weak button] _ in
        guard let self, let button else { return }
        button.configuration = self.configuration(for: key, selected: button.isSelected)
      }
      button.addAction(
        UIAction { [weak self, weak button] _ in
          guard let self, let button else { return }
          self.pressed(key, button)
        }, for: .touchDown)
      for event: UIControl.Event in [.touchUpInside, .touchUpOutside, .touchCancel] {
        button.addAction(UIAction { [weak self] _ in self?.stopRepeating() }, for: event)
      }
      buttons[key.id] = button
      return button
    }

    /// The key as drawn: inset inside its press area, so a finger lands anywhere in the 44 points.
    private func configuration(for key: PhoneTerminalKey, selected: Bool)
      -> UIButton.Configuration
    {
      var configuration = UIButton.Configuration.plain()
      let ink = selected ? colors.onSelected : colors.ink
      switch key.face {
      case .title(let text):
        configuration.attributedTitle = AttributedString(
          text,
          attributes: AttributeContainer([
            .font: UIFont.systemFont(ofSize: DesignTokens.TypeScale.control, weight: .semibold),
            .foregroundColor: ink,
          ]))
      case .symbol(let name):
        configuration.image = UIImage(systemName: name)
        configuration.baseForegroundColor = ink
      }
      configuration.contentInsets = .zero
      var background = UIBackgroundConfiguration.clear()
      background.backgroundColor = selected ? colors.selected : colors.key
      background.cornerRadius = DesignTokens.Radius.compact
      background.backgroundInsets = NSDirectionalEdgeInsets(
        top: DesignTokens.Space.x4, leading: DesignTokens.Space.x2,
        bottom: DesignTokens.Space.x4, trailing: DesignTokens.Space.x2)
      configuration.background = background
      return configuration
    }

    // MARK: Keys

    private func pressed(_ key: PhoneTerminalKey, _ button: UIButton) {
      UIDevice.current.playInputClick()
      guard let terminal else { return }
      switch key.action {
      case .bytes(let bytes): terminal.send(bytes)
      case .text(let text): terminal.insertText(text)
      case .function(let number): terminal.send(EscapeSequences.cmdF[number - 1])
      case .control:
        terminal.controlModifier.toggle()
        button.isSelected = terminal.controlModifier
      case .mouseReporting:
        terminal.allowMouseReporting.toggle()
        button.isSelected = !terminal.allowMouseReporting
      case .dismissKeyboard: _ = terminal.resignFirstResponder()
      case .arrow(let direction):
        let send: () -> Void = { [weak terminal] in
          guard let terminal else { return }
          terminal.send(Self.arrow(direction, in: terminal))
        }
        send()
        startRepeating(send)
      }
    }

    private static func arrow(_ direction: PhoneTerminalKey.Direction, in terminal: TerminalView)
      -> [UInt8]
    {
      let application = terminal.getTerminal().applicationCursor
      switch direction {
      case .up: return application ? EscapeSequences.moveUpApp : EscapeSequences.moveUpNormal
      case .down: return application ? EscapeSequences.moveDownApp : EscapeSequences.moveDownNormal
      case .left: return application ? EscapeSequences.moveLeftApp : EscapeSequences.moveLeftNormal
      case .right:
        return application ? EscapeSequences.moveRightApp : EscapeSequences.moveRightNormal
      }
    }

    private func startRepeating(_ send: @escaping () -> Void) {
      stopRepeating()
      let start = DispatchWorkItem { [weak self] in
        self?.repeatTimer = Timer.scheduledTimer(
          withTimeInterval: Self.repeatEvery, repeats: true
        ) { _ in send() }
      }
      repeatDelay = start
      DispatchQueue.main.asyncAfter(deadline: .now() + Self.repeatAfter, execute: start)
    }

    private func stopRepeating() {
      repeatDelay?.cancel()
      repeatDelay = nil
      repeatTimer?.invalidate()
      repeatTimer = nil
    }
  }
#endif
