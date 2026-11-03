#if os(iOS)
  import UIKit

  /// Handwriting with Apple Pencil (Scribble) over a view that is one writable area but is not a
  /// text field: the terminal. SwiftUI text fields and `UITextView` already take Scribble; this
  /// is the one place that makes a custom text input take it too, so a person can write on a
  /// terminal without tapping it first, as in any other field of the app.
  ///
  /// The whole view is a single element. Writing on it makes the view the first responder (the
  /// keyboard focus, which on a pane board also selects the pane) and the system then inserts
  /// the recognised text through the view's text input.
  @MainActor
  final class HandwritingArea: NSObject,
    @preconcurrency UIIndirectScribbleInteractionDelegate
  {
    typealias ElementIdentifier = String
    private static let element = "text"

    private weak var view: (UIView & UITextInput)?

    /// Makes `view` writable with the Pencil over its whole bounds. The interaction holds its
    /// delegate weakly, so the view keeps the returned area for as long as it lives.
    static func install(on view: UIView & UITextInput) -> HandwritingArea {
      let area = HandwritingArea(view: view)
      view.addInteraction(UIIndirectScribbleInteraction(delegate: area))
      return area
    }

    private init(view: UIView & UITextInput) { self.view = view }

    func indirectScribbleInteraction(
      _ interaction: UIInteraction, requestElementsIn rect: CGRect,
      completion: @escaping ([ElementIdentifier]) -> Void
    ) {
      completion(view.map { $0.bounds.intersects(rect) } == true ? [Self.element] : [])
    }

    func indirectScribbleInteraction(
      _ interaction: UIInteraction, isElementFocused elementIdentifier: ElementIdentifier
    ) -> Bool {
      view?.isFirstResponder ?? false
    }

    func indirectScribbleInteraction(
      _ interaction: UIInteraction, frameForElement elementIdentifier: ElementIdentifier
    ) -> CGRect {
      view?.bounds ?? .zero
    }

    func indirectScribbleInteraction(
      _ interaction: UIInteraction, focusElementIfNeeded elementIdentifier: ElementIdentifier,
      referencePoint focusReferencePoint: CGPoint,
      completion: @escaping ((UIResponder & UITextInput)?) -> Void
    ) {
      guard let view else { return completion(nil) }
      completion(view.isFirstResponder || view.becomeFirstResponder() ? view : nil)
    }
  }
#endif
