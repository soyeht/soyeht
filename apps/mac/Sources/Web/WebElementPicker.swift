import Foundation
import Observation
import SoyehtClient
import SoyehtProtocol
import WebKit

struct WebElementSelection: Sendable {
  let package: WebElementPackage
  let images: [WebHostAttachment]
}

private struct PickEvent: Decodable {
  let kind: String
  let element: PickedMetadata?
}

private struct PickedMetadata: Decodable {
  let selector: String
  let path: [String]
  let outerHtml: String
  let styles: WebComputedStyles
  let accessibility: WebAccessibility
  let viewportRect: WebRect
  let pageRect: WebRect
  let component: WebComponentSource?
}

@MainActor @Observable final class WebElementPicker {
  private let webView: WKWebView
  private var polling: Task<Void, Never>?
  private(set) var isActive = false
  private(set) var elements: [WebPickedElement] = []
  private(set) var images: [WebHostAttachment] = []

  init(webView: WKWebView) { self.webView = webView }

  var selection: WebElementSelection? {
    guard !elements.isEmpty, elements.count == images.count,
      let url = webView.url?.absoluteString
    else { return nil }
    return WebElementSelection(
      package: WebElementPackage(
        url: String(url.prefix(4_096)), title: String((webView.title ?? "").prefix(1_024)),
        elements: elements),
      images: images)
  }

  func toggle() {
    if isActive { deactivate(clear: false) } else { activate() }
  }

  func clear() {
    elements = []
    images = []
  }

  func takeSelection() -> WebElementSelection? {
    let value = selection
    deactivate(clear: true)
    return value
  }

  func navigationStarted() {
    deactivate(clear: true)
  }

  private func activate() {
    clear()
    isActive = true
    polling?.cancel()
    polling = Task { [weak self] in
      guard let self else { return }
      do {
        _ = try await webView.callAsyncJavaScript(
          Self.installScript, arguments: [:], in: nil, contentWorld: .page)
        try await poll()
      } catch {
        deactivate(clear: true)
      }
    }
  }

  private func deactivate(clear: Bool) {
    polling?.cancel()
    polling = nil
    isActive = false
    Task { @MainActor [weak self] in await self?.removeFromPage() }
    if clear { self.clear() }
  }

  private func removeFromPage() async {
    do {
      let result = try await webView.callAsyncJavaScript(
        Self.removeScript, arguments: [:], in: nil, contentWorld: .page)
      _ = result as? Bool
    } catch {}
  }

  private func poll() async throws {
    while !Task.isCancelled, isActive {
      try await Task.sleep(for: .milliseconds(80))
      let events: [PickEvent] = try await decoded(Self.drainScript)
      for event in events {
        if event.kind == "cancelled" || event.kind == "finished" {
          deactivate(clear: false)
          return
        }
        if let element = event.element { try await append(element) }
      }
    }
  }

  private func append(_ raw: PickedMetadata) async throws {
    guard elements.count < 20 else {
      deactivate(clear: false)
      return
    }
    let image = try await WebScreenshotCapture.capture(
      webView: webView, viewportRect: raw.viewportRect)
    let element = WebPickedElement(
      selector: raw.selector, path: raw.path, outerHtml: raw.outerHtml,
      styles: raw.styles, accessibility: raw.accessibility,
      viewportRect: raw.viewportRect, pageRect: raw.pageRect,
      image: image.reference, component: raw.component)
    elements.append(element)
    images.append(image)
  }

  private func decoded<T: Decodable>(_ script: String) async throws -> T {
    let raw = try await webView.callAsyncJavaScript(
      script, arguments: [:], in: nil, contentWorld: .page)
    guard let json = raw as? String, let data = json.data(using: .utf8) else {
      throw ClientFailure.webHost(.internal_error, "The selected element is invalid.")
    }
    return try JSONDecoder().decode(T.self, from: data)
  }
}

extension WebElementPicker {
  fileprivate static let drainScript = #"""
    return JSON.stringify(globalThis.__soyehtPicker?.drain() || []);
    """#

  fileprivate static let removeScript = #"""
    globalThis.__soyehtPicker?.remove();
    delete globalThis.__soyehtPicker;
    return true;
    """#

  fileprivate static let installScript = #"""
    globalThis.__soyehtPicker?.remove();
    const queue = [];
    let hovered = null;
    let oldOutline = "";
    let oldOffset = "";
    const bytes = (value, limit) => {
      const encoder = new TextEncoder();
      if (encoder.encode(value).byteLength <= limit) return value;
      let low = 0, high = value.length;
      while (low < high) {
        const mid = Math.ceil((low + high) / 2);
        if (encoder.encode(value.slice(0, mid)).byteLength <= limit) low = mid;
        else high = mid - 1;
      }
      return value.slice(0, low);
    };
    const escape = value => CSS.escape(String(value));
    const unique = selector => {
      try { return document.querySelectorAll(selector).length === 1; }
      catch (_) { return false; }
    };
    const selector = element => {
      if (element.id) {
        const candidate = `#${escape(element.id)}`;
        if (unique(candidate)) return candidate;
      }
      const parts = [];
      for (let node = element; node && node.nodeType === 1; node = node.parentElement) {
        let part = node.tagName.toLowerCase();
        const classes = Array.from(node.classList).slice(0, 4).map(value => `.${escape(value)}`);
        if (classes.length) part += classes.join("");
        let candidate = [part, ...parts].join(" > ");
        if (!unique(candidate)) {
          const siblings = node.parentElement
            ? Array.from(node.parentElement.children).filter(value => value.tagName === node.tagName)
            : [];
          if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
          candidate = [part, ...parts].join(" > ");
        }
        parts.unshift(part);
        if (unique(candidate)) return candidate;
      }
      return parts.join(" > ");
    };
    const path = element => {
      const parts = [];
      for (let node = element; node && node.nodeType === 1; node = node.parentElement) {
        parts.unshift(bytes(node.tagName.toLowerCase() + Array.from(node.classList).slice(0, 6)
          .map(value => `.${value}`).join(""), 512));
      }
      return parts.slice(-64);
    };
    const role = element => element.getAttribute("role") || ({A:"link",BUTTON:"button",
      INPUT:element.type === "checkbox" ? "checkbox" : "textbox",SELECT:"combobox",
      TEXTAREA:"textbox",SUMMARY:"button"}[element.tagName] || element.tagName.toLowerCase());
    const accessibility = element => ({
      role: bytes(role(element), 256),
      name: bytes(String(element.getAttribute("aria-label") || element.alt || element.title ||
        element.innerText || "").trim(), 2048),
      state: bytes([
        element.disabled ? "disabled" : "enabled",
        element.checked === true ? "checked" : "",
        element.selected === true ? "selected" : "",
        element.required === true ? "required" : "",
        element.getAttribute("aria-expanded") ? `expanded=${element.getAttribute("aria-expanded")}` : "",
        element.getAttribute("aria-invalid") ? `invalid=${element.getAttribute("aria-invalid")}` : ""
      ].filter(Boolean).join(" "), 2048)
    });
    const component = element => {
      const keys = Object.getOwnPropertyNames(element);
      const fiberKey = keys.find(key => key.startsWith("__reactFiber$"));
      if (fiberKey) {
        for (let fiber = element[fiberKey]; fiber; fiber = fiber.return) {
          const type = fiber.elementType || fiber.type;
          const name = type?.displayName || type?.name;
          const source = fiber._debugSource;
          if (name) return {framework:"React",component:bytes(String(name),512),
            ...(source?.fileName ? {sourceFile:bytes(String(source.fileName),4096)} : {}),
            ...(source?.lineNumber > 0 ? {line:Math.min(4294967295,source.lineNumber)} : {})};
        }
      }
      const vue = element.__vueParentComponent;
      const inspector = element.getAttribute("data-v-inspector");
      if (vue || inspector) {
        const file = vue?.type?.__file || inspector?.replace(/:\d+:\d+$/, "");
        const line = inspector?.match(/:(\d+):\d+$/)?.[1];
        return {framework:"Vue",component:bytes(String(vue?.type?.name || file?.split("/").pop() || "component"),512),
          ...(file ? {sourceFile:bytes(String(file),4096)} : {}),
          ...(line ? {line:Number(line)} : {})};
      }
      const svelte = element.__svelte_meta;
      if (svelte) return {framework:"Svelte",component:bytes(String(svelte.component || "component"),512),
        ...(svelte.loc?.file ? {sourceFile:bytes(String(svelte.loc.file),4096)} : {}),
        ...(svelte.loc?.line > 0 ? {line:Number(svelte.loc.line)} : {})};
      return null;
    };
    const capture = element => {
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      const bounded = value => Math.max(-2147483648, Math.min(2147483647, Math.round(value)));
      const size = value => Math.max(0, Math.min(16384, Math.round(value)));
      return {
        selector: bytes(selector(element), 4096), path: path(element),
        outerHtml: bytes(element.outerHTML || "", 2048),
        styles: {boxSizing:bytes(style.boxSizing,256),color:bytes(style.color,256),
          background:bytes(style.background,512),font:bytes(style.font,512),
          border:bytes(style.border,512),borderRadius:bytes(style.borderRadius,256),
          display:bytes(style.display,256),position:bytes(style.position,256)},
        accessibility: accessibility(element),
        viewportRect:{x:bounded(rect.x),y:bounded(rect.y),width:size(rect.width),height:size(rect.height)},
        pageRect:{x:bounded(rect.x + scrollX),y:bounded(rect.y + scrollY),width:size(rect.width),height:size(rect.height)},
        ...(component(element) ? {component:component(element)} : {})
      };
    };
    const restore = () => {
      if (!hovered) return;
      hovered.style.outline = oldOutline;
      hovered.style.outlineOffset = oldOffset;
      hovered = null;
    };
    const move = event => {
      if (!(event.target instanceof Element) || event.target === hovered) return;
      restore(); hovered = event.target; oldOutline = hovered.style.outline;
      oldOffset = hovered.style.outlineOffset;
      hovered.style.outline = "2px solid #5B7CFA"; hovered.style.outlineOffset = "2px";
    };
    const click = event => {
      event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation();
      if (!(event.target instanceof Element) || queue.length >= 20) return;
      queue.push({kind:"picked",element:capture(event.target)});
      if (!event.shiftKey) { queue.push({kind:"finished"}); picker.remove(); }
    };
    const key = event => {
      if (event.key !== "Escape") return;
      event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation();
      queue.push({kind:"cancelled"}); picker.remove();
    };
    const picker = {
      drain() { return queue.splice(0, queue.length); },
      remove() { restore(); document.removeEventListener("pointerover",move,true);
        document.removeEventListener("click",click,true); document.removeEventListener("keydown",key,true); }
    };
    document.addEventListener("pointerover",move,true);
    document.addEventListener("click",click,true);
    document.addEventListener("keydown",key,true);
    globalThis.__soyehtPicker = picker;
    return true;
    """#
}
