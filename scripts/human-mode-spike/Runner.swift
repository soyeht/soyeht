import AppKit
import WebKit

final class Runner: NSObject, NSApplicationDelegate, WKNavigationDelegate, WKUIDelegate {
  let app = NSApplication.shared
  var window: NSWindow!
  var web: WKWebView!
  let expectedUA =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.4 Safari/605.1.15"
  let delivery = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "process"
  let output = URL(
    fileURLWithPath: CommandLine.arguments.count > 2
      ? CommandLine.arguments[2] : "/tmp/human-mode-spike.json")
  let poster = ProcessInfo.processInfo.environment["SOYEHT_HUMAN_SPIKE_POSTER"] ?? ""
  let originalCursor = NSEvent.mouseLocation

  func applicationDidFinishLaunching(_ notification: Notification) {
    let config = WKWebViewConfiguration()
    web = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 600), configuration: config)
    web.customUserAgent = expectedUA
    web.navigationDelegate = self
    web.uiDelegate = self
    window = NSWindow(
      contentRect: NSRect(x: 120, y: 120, width: 800, height: 600),
      styleMask: [.titled, .closable], backing: .buffered, defer: false)
    window.contentView = web
    window.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
    web.load(URLRequest(url: URL(string: "http://127.0.0.1:18765/")!))
  }

  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.runMouse() }
  }

  func webView(
    _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
    for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
  ) -> WKWebView? {
    WKWebView(frame: .zero, configuration: configuration)
  }

  func runMouse() {
    web.evaluateJavaScript(
      "(()=>{let r=target.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}})()"
    ) { value, error in
      guard let p = value as? [String: Any], let x = p["x"] as? Double, let y = p["y"] as? Double
      else {
        self.fail("target rect: \(String(describing:error))")
        return
      }
      let point = self.globalPoint(x: x, y: y)
      self.sendMouse(.mouseMoved, at: point, clickCount: 0)
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) {
        self.sendMouse(.leftMouseDown, at: point, clickCount: 1)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.07) {
          self.sendMouse(.leftMouseUp, at: point, clickCount: 1)
          DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { self.focusInput() }
        }
      }
    }
  }

  func focusInput() {
    web.evaluateJavaScript(
      "(()=>{let r=typing.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}})()"
    ) { value, error in
      guard let p = value as? [String: Any], let x = p["x"] as? Double, let y = p["y"] as? Double
      else {
        self.fail("input rect: \(String(describing:error))")
        return
      }
      let point = self.globalPoint(x: x, y: y)
      self.sendMouse(.leftMouseDown, at: point, clickCount: 1)
      self.sendMouse(.leftMouseUp, at: point, clickCount: 1)
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
        self.type(Array("human"), delays: [0.06, 0.13, 0.08, 0.17, 0.10], index: 0)
      }
    }
  }

  func type(_ chars: [Character], delays: [Double], index: Int) {
    guard index < chars.count else {
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.finish() }
      return
    }
    let codes: [CGKeyCode] = [4, 32, 46, 0, 45]
    post(["\(getpid())", delivery, "key", "\(codes[index])", String(chars[index])])
    DispatchQueue.main.asyncAfter(deadline: .now() + delays[index]) {
      self.type(chars, delays: delays, index: index + 1)
    }
  }

  func globalPoint(x: Double, y: Double) -> CGPoint {
    let inWindow = web.convert(NSPoint(x: x, y: web.isFlipped ? y : web.bounds.height - y), to: nil)
    let onScreen = window.convertPoint(toScreen: inWindow)
    return CGPoint(x: onScreen.x, y: (NSScreen.screens.first?.frame.maxY ?? 0) - onScreen.y)
  }

  func sendMouse(_ eventType: NSEvent.EventType, at point: NSPoint, clickCount: Int) {
    let kind: CGEventType =
      switch eventType {
      case .mouseMoved: .mouseMoved
      case .leftMouseDown: .leftMouseDown
      default: .leftMouseUp
      }
    post([
      "\(getpid())", delivery, "mouse", "\(kind.rawValue)", "\(point.x)", "\(point.y)",
      "\(clickCount)",
    ])
  }

  func post(_ arguments: [String]) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: poster)
    process.arguments = arguments
    do {
      try process.run()
      process.waitUntilExit()
    } catch { fail("poster: \(error)") }
  }

  func finish() {
    web.evaluateJavaScript("result()") { value, error in
      guard var result = value as? [String: Any] else {
        self.fail("result: \(String(describing:error))")
        return
      }
      let times = result["keyTimes"] as? [Double] ?? []
      let intervals = zip(times.dropFirst(), times).map { $0 - $1 }
      let events = result["events"] as? [String] ?? []
      result["pointerMoveBeforeClick"] =
        (events.firstIndex(of: "pointermove") ?? 999) < (events.firstIndex(of: "click") ?? -1)
      result["typingVaries"] =
        intervals.count >= 3 && ((intervals.max() ?? 0) - (intervals.min() ?? 0) > 20)
      result["userAgentMatchesSafari"] = (result["userAgent"] as? String) == self.expectedUA
      let webdriver = result["webdriver"] as? String
      result["noAutomationFlag"] =
        (webdriver == "undefined" || webdriver == "false")
        && (result["automationProperty"] as? Bool) == false
      result["deliveryPath"] = self.delivery
      result["visibleCursorStayedPut"] = self.delivery == "process"
      if self.delivery == "global" {
        CGWarpMouseCursorPosition(
          CGPoint(
            x: self.originalCursor.x,
            y: (NSScreen.screens.first?.frame.maxY ?? 0) - self.originalCursor.y))
      }
      do {
        let data = try JSONSerialization.data(
          withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: self.output)
        print(String(data: data, encoding: .utf8)!)
        self.app.terminate(nil)
      } catch { self.fail("write: \(error)") }
    }
  }

  func fail(_ message: String) {
    fputs("FAIL \(message)\n", stderr)
    app.terminate(nil)
  }
}
let runner = Runner()
NSApplication.shared.setActivationPolicy(.regular)
NSApplication.shared.delegate = runner
NSApplication.shared.run()
