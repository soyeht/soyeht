#if os(macOS)
  import AppKit
  import Foundation
  import SoyehtProtocol
  import Testing
  import WebKit

  @testable import SoyehtClient

  /// A page that tries to notice the app around it (ADR 0109). It records, from the first line of
  /// its own script, every change to its own world: unknown globals, message handlers, patched
  /// natives, added nodes, attributes and untrusted events. `__detect()` reports them.
  private let detectorScript = #"""
    const known = new Set(["__detect", "__log"]);
    const log = { mutations: [], untrusted: [], trusted: {}, };
    window.__log = log;
    new MutationObserver(records => {
      for (const record of records) {
        log.mutations.push(record.type + ":" + (record.target.nodeName || "") + ":" +
          (record.attributeName || "") + ":" + record.addedNodes.length);
      }
    }).observe(document, {subtree: true, childList: true, attributes: true, characterData: true});
    for (const name of ["click", "dblclick", "mousedown", "mouseup", "mouseover", "mouseenter",
      "mousemove", "pointerdown", "pointerup", "pointerover", "keydown", "keyup", "keypress",
      "input", "change", "focus", "blur", "wheel", "scroll", "touchstart", "submit"]) {
      addEventListener(name, event => {
        if (event.isTrusted) log.trusted[name] = (log.trusted[name] || 0) + 1;
        else log.untrusted.push(name);
      }, {capture: true, passive: true});
    }
    window.__detect = () => {
      const findings = [];
      const frame = document.createElement("iframe");
      frame.style.display = "none";
      document.documentElement.appendChild(frame);
      const pristine = frame.contentWindow;
      const pristineNames = new Set(Object.getOwnPropertyNames(pristine));
      const pristineToString = pristine.Function.prototype.toString;
      for (const name of Object.getOwnPropertyNames(window)) {
        // window[0] is the probe iframe itself.
        if (!/^\d+$/.test(name) && !pristineNames.has(name) && !known.has(name))
          findings.push("global:" + name);
        if (/soyeht/i.test(name)) findings.push("soyeht-global:" + name);
      }
      // A view with no script message handler has no `window.webkit` at all.
      if (typeof window.webkit !== "undefined") findings.push("webkit-namespace");
      if (window.webkit && window.webkit.messageHandlers) findings.push("message-handlers");
      const natives = {
        "console.log": console.log, "console.info": console.info, "console.warn": console.warn,
        "console.error": console.error, "console.debug": console.debug, "fetch": window.fetch,
        "XMLHttpRequest.open": XMLHttpRequest.prototype.open,
        "XMLHttpRequest.send": XMLHttpRequest.prototype.send,
        "Function.toString": Function.prototype.toString,
        "addEventListener": EventTarget.prototype.addEventListener,
        "dispatchEvent": EventTarget.prototype.dispatchEvent,
        "scrollTo": window.scrollTo, "getComputedStyle": window.getComputedStyle,
        "querySelectorAll": Document.prototype.querySelectorAll,
        "createRange": Document.prototype.createRange,
      };
      for (const [name, fn] of Object.entries(natives)) {
        const text = pristineToString.call(fn);
        if (!/\{\s*\[native code\]\s*\}$/.test(text)) findings.push("not-native:" + name);
        if (Function.prototype.toString.call(fn) !== text) findings.push("toString-differs:" + name);
      }
      if (console.log.hasOwnProperty("prototype")) findings.push("console-has-prototype");
      if (Object.keys(window.XMLHttpRequest.prototype).some(key => /soyeht/i.test(key)))
        findings.push("xhr-key");
      for (const record of log.mutations) findings.push("mutation:" + record);
      for (const name of log.untrusted) findings.push("untrusted-event:" + name);
      frame.remove();
      return JSON.stringify({findings, trusted: log.trusted, ua: navigator.userAgent,
        webdriver: navigator.webdriver === true});
    };
    """#

  private struct DetectorReport: Decodable {
    var findings: [String]
    var trusted: [String: Int]
    var ua: String
    var webdriver: Bool
  }

  @MainActor private func readerFixture(base: String) async -> WebPageFixture {
    let fixture = WebPageFixture()
    await fixture.load(WebTestPages.tall(extraScript: detectorScript), baseURL: base)
    return fixture
  }

  @MainActor private func report(_ fixture: WebPageFixture) async throws -> DetectorReport {
    let json = try await fixture.pageValue("return window.__detect()")
    return try JSONDecoder().decode(DetectorReport.self, from: Data(json.utf8))
  }

  @Test @MainActor func aPageOnAnotherSiteFindsNothingWhileTheAppReadsIt() async throws {
    let fixture = await readerFixture(base: "https://detector.example/")
    #expect(!fixture.observation.isInstalled)

    _ = try await fixture.actions.snapshot(generation: 1)
    _ = try await fixture.observation.wait(
      args: .object(["text": .string("Block 3"), "networkIdle": .bool(true)]), timeoutMs: 5_000)
    let capture = try await fixture.read(screens: 6)
    #expect(capture.screens > 1)

    let result = try await report(fixture)
    #expect(result.findings.isEmpty, "\(result.findings)")
    #expect(!result.webdriver)
    #expect(!result.ua.localizedCaseInsensitiveContains("soyeht"))
    #expect(
      result.ua.contains(" Version/") && result.ua.hasSuffix(" Safari/605.1.15"), "\(result.ua)")
    // The scrolling a read does is the view's own: the page sees trusted scroll events and no
    // key, wheel, pointer or focus event.
    #expect((result.trusted["scroll"] ?? 0) > 0)
    #expect(result.trusted.keys.sorted() == ["scroll"], "\(result.trusted)")
  }

  @Test @MainActor func theDetectorSeesTheCaptureWhereTheAppDeliberatelyInstallsIt() async throws {
    let fixture = await readerFixture(base: "http://127.0.0.1:5173/")
    #expect(fixture.observation.isInstalled)

    let result = try await report(fixture)
    #expect(result.findings.contains("webkit-namespace"))
    #expect(result.findings.contains("message-handlers"))
    #expect(result.findings.contains { $0.hasPrefix("not-native:console") })
    #expect(result.findings.contains("not-native:fetch"))
  }

  @Test @MainActor func leavingADevelopmentHostTakesTheCaptureOffBeforeTheNextPage() async throws {
    let fixture = await readerFixture(base: "http://localhost:3000/")
    #expect(fixture.observation.isInstalled)

    await fixture.load(
      WebTestPages.tall(extraScript: detectorScript), baseURL: "https://elsewhere.example/")
    #expect(!fixture.observation.isInstalled)
    let result = try await report(fixture)
    #expect(result.findings.isEmpty, "\(result.findings)")
  }

  @Test @MainActor func anAgentActionIsTheOneThingTheDetectorSeesBecauseItIsAnEvent() async throws {
    let fixture = WebPageFixture()
    await fixture.load(
      WebTestPages.page(body: "<button>Go</button>", script: detectorScript),
      baseURL: "https://detector.example/")

    let snapshot = try await fixture.actions.snapshot(generation: 1)
    guard case .object(let fields) = snapshot, case .array(let nodes)? = fields["nodes"],
      let button = nodes.compactMap({ node -> String? in
        guard case .object(let values) = node, case .string("Go")? = values["name"],
          case .string(let ref)? = values["ref"]
        else { return nil }
        return ref
      }).first
    else {
      Issue.record("the snapshot has no button")
      return
    }
    try await fixture.actions.perform(
      .click, args: .object(["ref": .string(button)]), generation: 1)

    let result = try await report(fixture)
    #expect(result.findings == ["untrusted-event:click"])
  }

  @Test @MainActor func theDetectorSeesAGlobalAndANodeAddedToThePage() async throws {
    let fixture = WebPageFixture()
    await fixture.load(
      WebTestPages.tall(extraScript: detectorScript), baseURL: "https://detector.example/")
    _ = try await fixture.pageValue(
      "window.__marker = 1; document.body.setAttribute('data-x', '1'); document.body.append(document.createElement('i')); return ''"
    )

    let result = try await report(fixture)
    #expect(result.findings.contains("global:__marker"))
    #expect(result.findings.contains { $0.hasPrefix("mutation:attributes:BODY:data-x") })
    #expect(result.findings.contains { $0.hasPrefix("mutation:childList:BODY") })
  }
#endif
