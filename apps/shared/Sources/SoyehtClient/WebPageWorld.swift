#if os(macOS)
  import WebKit

  /// Where the app runs its own scripts in a web pane.
  ///
  /// An isolated content world has its own JavaScript globals and wrappers over the same
  /// document: the page cannot see a global, a patched function or a handler that lives there
  /// (ADR 0109). Only a feature that must change or read the page's own objects, such as
  /// `web.evaluate`, runs in `WKContentWorld.page`.
  @MainActor public enum WebPageWorld {
    public static let isolated = WKContentWorld.world(name: "com.soyeht.web")
  }
#endif
