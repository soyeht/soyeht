import SoyehtProtocol

/// Which slide a line of a presentation's source belongs to. `contracts/vectors/presentation.json`
/// holds the cases, which the desktop client passes as well.
public enum WritingSlideSelection {
  /// The slide whose lines hold `line`. The engine's `endLine` is the line after the slide's last
  /// one, and it counts as the slide's own: the caret at the end of a slide is still on it.
  public static func index(containing line: UInt32, in slides: [WritingSlide]) -> UInt32? {
    slides.first { line >= $0.startLine && line <= $0.endLine }?.index
  }
}
