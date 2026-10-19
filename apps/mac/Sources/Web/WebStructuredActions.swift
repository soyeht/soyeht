import Foundation
import SoyehtClient
import SoyehtProtocol
import WebKit

private struct SnapshotCapture: Decodable {
  var nodes: [SnapshotCaptureNode]
  var truncated: Bool
}

private struct SnapshotCaptureNode: Decodable {
  var ref: String
  var role: String
  var name: String
  var state: String
  var value: String?
  var rect: WebRect
  var parentRef: String?
}

private struct ActionOutcome: Decodable {
  var completed: Bool
  var stale: Bool
}

@MainActor final class WebStructuredActions {
  private let webView: WKWebView
  private let world = WKContentWorld.world(name: "com.soyeht.web.structured")
  private var acceptedReferences: Set<String> = []
  private var acceptedGeneration: UInt64?

  init(webView: WKWebView) { self.webView = webView }

  func navigationStarted() {
    acceptedReferences.removeAll(keepingCapacity: true)
    acceptedGeneration = nil
  }

  func snapshot(generation: UInt64) async throws -> JsonValue {
    let capture: SnapshotCapture = try await decoded(
      script: Self.snapshotScript, arguments: ["generation": String(generation)])
    acceptedGeneration = generation
    acceptedReferences = Set(capture.nodes.map(\.ref))
    return try boundedSnapshot(capture, generation: generation)
  }

  func perform(_ command: WebCommandKind, args: JsonValue, generation: UInt64) async throws {
    if let reference = string(args, "ref") {
      guard acceptedGeneration == generation, acceptedReferences.contains(reference) else {
        throw ClientFailure.webHost(.stale_reference, "Take a new snapshot and try again.")
      }
    }
    let outcome: ActionOutcome = try await decoded(
      script: Self.actionScript,
      arguments: [
        "command": command.rawValue,
        "generation": String(generation),
        "payload": try jsonObject(args),
      ])
    if outcome.stale {
      throw ClientFailure.webHost(.stale_reference, "Take a new snapshot and try again.")
    }
    guard outcome.completed else {
      throw ClientFailure.webHost(.internal_error, "The page action was refused.")
    }
  }

  func evaluate(_ args: JsonValue) async throws -> String {
    guard let source = string(args, "js"), !source.isEmpty,
      source.utf8.count <= ProtocolLimits.webEvaluateBytes
    else {
      throw ClientFailure.webHost(.internal_error, "The evaluation source is invalid.")
    }
    let raw = try await webView.callAsyncJavaScript(
      Self.evaluateScript,
      arguments: ["source": source, "maxBytes": ProtocolLimits.webEvaluateBytes],
      in: nil, contentWorld: .page)
    guard let value = raw as? String,
      value.utf8.count <= ProtocolLimits.webEvaluateBytes
    else {
      throw ClientFailure.webHost(.internal_error, "The evaluation result exceeds its limit.")
    }
    return value
  }

  private func boundedSnapshot(_ capture: SnapshotCapture, generation: UInt64) throws -> JsonValue {
    var encodedNodes: [JsonValue] = []
    var accepted: Set<String> = []
    var truncated = capture.truncated
    var encodedBytes = try JSONEncoder().encode(
      snapshotValue(generation: generation, nodes: [], truncated: false)
    ).count
    for raw in capture.nodes {
      let node = WebSnapshotNode(
        ref: raw.ref, role: raw.role, name: raw.name, state: raw.state, value: raw.value,
        rect: raw.rect, parentRef: raw.parentRef.flatMap { accepted.contains($0) ? $0 : nil })
      let encoded = try jsonValue(node)
      let nodeBytes = try JSONEncoder().encode(encoded).count + (encodedNodes.isEmpty ? 0 : 1)
      if encodedBytes + nodeBytes > ProtocolLimits.webSnapshotBytes - 512 {
        truncated = true
        break
      }
      encodedNodes.append(encoded)
      accepted.insert(raw.ref)
      encodedBytes += nodeBytes
    }
    acceptedReferences = accepted
    return snapshotValue(generation: generation, nodes: encodedNodes, truncated: truncated)
  }

  private func snapshotValue(
    generation: UInt64, nodes: [JsonValue], truncated: Bool
  ) -> JsonValue {
    .object([
      "navigationGeneration": .string(String(generation)),
      "title": .string(String((webView.title ?? "").prefix(ProtocolLimits.webTitleBytes))),
      "url": .string(
        String((webView.url?.absoluteString ?? "").prefix(ProtocolLimits.webFieldBytes))),
      "nodes": .array(nodes), "truncated": .bool(truncated),
    ])
  }

  private func decoded<T: Decodable>(
    script: String, arguments: [String: Any]
  ) async throws -> T {
    let raw = try await webView.callAsyncJavaScript(
      script, arguments: arguments, in: nil, contentWorld: world)
    guard let json = raw as? String, let data = json.data(using: .utf8) else {
      throw ClientFailure.webHost(.internal_error, "The page returned an invalid result.")
    }
    do { return try JSONDecoder().decode(T.self, from: data) } catch {
      throw ClientFailure.webHost(.internal_error, "The page returned an invalid result.")
    }
  }

  private func jsonObject(_ value: JsonValue) throws -> Any {
    try JSONSerialization.jsonObject(with: JSONEncoder().encode(value))
  }

  private func jsonValue<T: Encodable>(_ value: T) throws -> JsonValue {
    try JSONDecoder().decode(JsonValue.self, from: JSONEncoder().encode(value))
  }

  private func string(_ args: JsonValue, _ key: String) -> String? {
    guard case .object(let values) = args, case .string(let value)? = values[key] else {
      return nil
    }
    return value
  }
}

extension WebStructuredActions {
  fileprivate static let evaluateScript = #"""
    const result = await (0, eval)(source);
    let output;
    if (typeof result === "string") output = result;
    else if (typeof result === "undefined") output = "undefined";
    else {
      try {
        const encoded = JSON.stringify(result);
        output = encoded === undefined ? String(result) : encoded;
      } catch (_) { output = String(result); }
    }
    if (new TextEncoder().encode(output).byteLength > maxBytes)
      throw new Error("evaluation result exceeds limit");
    return output;
    """#

  fileprivate static let snapshotScript = #"""
    const visible = element => {
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return style.visibility !== "hidden" && style.display !== "none" && rect.width > 0 && rect.height > 0;
    };
    const roleOf = element => element.getAttribute("role") || ({
      A:"link", BUTTON:"button", INPUT: element.type === "checkbox" ? "checkbox" : "textbox",
      SELECT:"combobox", TEXTAREA:"textbox", SUMMARY:"button", H1:"heading", H2:"heading",
      H3:"heading", H4:"heading", H5:"heading", H6:"heading", LABEL:"label"
    }[element.tagName] || element.tagName.toLowerCase());
    const textOf = (element, limit) => String(element.getAttribute("aria-label") ||
      element.alt || element.title || element.placeholder || element.innerText || "").trim().slice(0, limit);
    const stateOf = element => [
      element.disabled ? "disabled" : "",
      element.checked === true ? "checked" : "",
      element.selected === true ? "selected" : "",
      element.getAttribute("aria-expanded") ? `expanded=${element.getAttribute("aria-expanded")}` : ""
    ].filter(Boolean).join(" ").slice(0, \#(ProtocolLimits.webLabelBytes));
    const candidates = Array.from(document.querySelectorAll(
      "a,button,input,select,textarea,summary,[role],[aria-label],h1,h2,h3,h4,h5,h6,label"));
    const eligible = candidates.filter(element => visible(element) && (textOf(element, \#(ProtocolLimits.webLabelBytes)) ||
      ["INPUT","SELECT","TEXTAREA"].includes(element.tagName)));
    const selected = eligible.slice(0, 700);
    const refs = new Map();
    selected.forEach((element, index) => refs.set(element, `e${index + 1}`));
    globalThis.__soyehtReferences = new Map();
    globalThis.__soyehtGeneration = generation;
    const nodes = selected.map(element => {
      const ref = refs.get(element);
      globalThis.__soyehtReferences.set(ref, element);
      let parent = element.parentElement;
      while (parent && !refs.has(parent)) parent = parent.parentElement;
      const rect = element.getBoundingClientRect();
      const value = "value" in element ? String(element.value).slice(0, \#(ProtocolLimits.webFieldBytes)) : null;
      return {ref, role:roleOf(element).slice(0,\#(ProtocolLimits.webTokenBytes)), name:textOf(element,\#(ProtocolLimits.webLabelBytes)),
        state:stateOf(element), value,
        rect:{x:Math.max(-2147483648,Math.min(2147483647,Math.round(rect.x))),
          y:Math.max(-2147483648,Math.min(2147483647,Math.round(rect.y))),
          width:Math.max(0,Math.min(\#(ProtocolLimits.webExtentMax),Math.round(rect.width))),
          height:Math.max(0,Math.min(\#(ProtocolLimits.webExtentMax),Math.round(rect.height)))},
        parentRef:parent ? refs.get(parent) : null};
    });
    return JSON.stringify({nodes, truncated:eligible.length > selected.length});
    """#

  fileprivate static let actionScript = #"""
    const refs = globalThis.__soyehtReferences;
    const current = globalThis.__soyehtGeneration;
    const ref = payload.ref;
    if (String(current) !== String(generation) || (ref && (!refs || !refs.has(ref))))
      return JSON.stringify({completed:false,stale:true});
    const element = ref ? refs.get(ref) : null;
    const fire = (target, name) => target.dispatchEvent(new Event(name, {bubbles:true}));
    if (command === "click") { element.scrollIntoView({block:"center",inline:"center"}); element.click(); }
    else if (command === "hover") {
      element.scrollIntoView({block:"center",inline:"center"});
      element.dispatchEvent(new MouseEvent("mouseover", {bubbles:true,view:window}));
      element.dispatchEvent(new MouseEvent("mouseenter", {bubbles:false,view:window}));
    } else if (command === "type") {
      element.focus();
      if (element.isContentEditable) element.textContent = String(payload.text || "");
      else if ("value" in element) element.value = String(payload.text || "");
      else return JSON.stringify({completed:false,stale:false});
      fire(element,"input"); fire(element,"change");
      if (payload.submit === true) {
        element.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter",code:"Enter",bubbles:true}));
        if (element.form) element.form.requestSubmit();
        element.dispatchEvent(new KeyboardEvent("keyup",{key:"Enter",code:"Enter",bubbles:true}));
      }
    } else if (command === "press") {
      const target = document.activeElement || document.body;
      target.dispatchEvent(new KeyboardEvent("keydown",{key:String(payload.key),bubbles:true}));
      target.dispatchEvent(new KeyboardEvent("keyup",{key:String(payload.key),bubbles:true}));
    } else if (command === "select") {
      if (!(element instanceof HTMLSelectElement)) return JSON.stringify({completed:false,stale:false});
      element.value = String(payload.value || ""); fire(element,"input"); fire(element,"change");
    } else if (command === "scroll") {
      (element || window).scrollBy({left:Number(payload.dx || 0),top:Number(payload.dy || 0),behavior:"auto"});
    } else return JSON.stringify({completed:false,stale:false});
    return JSON.stringify({completed:true,stale:false});
    """#
}
