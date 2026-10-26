#if os(macOS)
  import SoyehtProtocol

  /// The scripts a read runs in the isolated world. They only read, except `scroll`, which moves
  /// the one scroller a native page-down cannot reach (an element inside a page that does not
  /// scroll itself) and puts the person's place back. No event is dispatched, no node is
  /// created and no global is left behind.
  enum WebReadScripts {
    /// The element a read scrolls when the document does not: the largest `overflow: auto|scroll`
    /// element that covers half of the view and holds more than it shows. Shared by both scripts.
    private static let findScroller = #"""
      const root = document.body || document.documentElement;
      const doc = document.documentElement;
      const view = {width: window.innerWidth, height: window.innerHeight};
      const documentScrolls = () => Math.max(doc.scrollHeight, root.scrollHeight || 0) > view.height + 1 &&
        !(getComputedStyle(doc).overflowY === "hidden" && getComputedStyle(root).overflowY === "hidden");
      const findScroller = () => {
        if (documentScrolls()) return null;
        let best = null, bestArea = 0, seen = 0;
        for (const element of document.querySelectorAll("body *")) {
          if (++seen > 20000) break;
          if (element.scrollHeight <= element.clientHeight + 1) continue;
          const overflow = getComputedStyle(element).overflowY;
          if (overflow !== "auto" && overflow !== "scroll") continue;
          const rect = element.getBoundingClientRect();
          const width = Math.min(rect.right, view.width) - Math.max(rect.left, 0);
          const height = Math.min(rect.bottom, view.height) - Math.max(rect.top, 0);
          if (width <= 0 || height <= 0 || width * height * 2 < view.width * view.height) continue;
          if (width * height > bestArea) { best = element; bestArea = width * height; }
        }
        return best;
      };
      """#

    static let state =
      findScroller + #"""
        const scroller = findScroller();
        const box = scroller ? (() => {
          const rect = scroller.getBoundingClientRect();
          const left = Math.max(rect.left, 0), top = Math.max(rect.top, 0);
          return {x: left, y: top, width: Math.min(rect.right, view.width) - left,
            height: Math.min(rect.bottom, view.height) - top};
        })() : {x: 0, y: 0, width: view.width, height: view.height};
        const state = {
          scrollY: scroller ? scroller.scrollTop : window.scrollY,
          viewHeight: scroller ? scroller.clientHeight : view.height,
          pageHeight: scroller ? scroller.scrollHeight : Math.max(doc.scrollHeight, root.scrollHeight || 0),
          inner: scroller !== null, box, topBand: 0, bottomBand: 0, cut: false, blocks: [], links: []
        };
        if (!collect) return JSON.stringify(state);
        const skipped = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD", "TITLE"]);
        const displays = new Map();
        const hidden = new Map();
        const pins = new Map();
        const displayOf = element => {
          if (!displays.has(element)) displays.set(element, getComputedStyle(element).display);
          return displays.get(element);
        };
        const blockOf = node => {
          for (let element = node.parentElement; element && element !== root; element = element.parentElement) {
            const display = displayOf(element);
            if (!display.startsWith("inline") && display !== "contents") return element;
          }
          return root;
        };
        const isHidden = element => {
          if (!hidden.has(element)) hidden.set(element, getComputedStyle(element).visibility !== "visible");
          return hidden.get(element);
        };
        const pinned = element => {
          const path = [];
          let result = false;
          for (let current = element; current; current = current.parentElement) {
            if (pins.has(current)) { result = pins.get(current); break; }
            path.push(current);
            const position = getComputedStyle(current).position;
            if (position === "fixed" || position === "sticky") { result = true; break; }
          }
          for (const current of path) pins.set(current, result);
          return result;
        };
        const onScreen = rect => rect.bottom > box.y && rect.top < box.y + box.height &&
          rect.right > box.x && rect.left < box.x + box.width;
        let examined = 0;
        for (const element of document.querySelectorAll("body *")) {
          if (++examined > 20000) { state.cut = true; break; }
          const position = getComputedStyle(element).position;
          if (position !== "fixed" && position !== "sticky") continue;
          const rect = element.getBoundingClientRect();
          if (rect.width < box.width / 2 || rect.height <= 0 || isHidden(element)) continue;
          if (rect.top <= box.y + 1 && rect.bottom <= box.y + box.height / 2)
            state.topBand = Math.max(state.topBand, rect.bottom - box.y);
          if (rect.bottom >= box.y + box.height - 1 && rect.top >= box.y + box.height / 2)
            state.bottomBand = Math.max(state.bottomBand, box.y + box.height - rect.top);
        }
        const groups = new Map();
        const range = document.createRange();
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        let visited = 0;
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          if (++visited > 40000) { state.cut = true; break; }
          const parent = node.parentElement;
          if (!parent || skipped.has(parent.tagName) || !/\S/.test(node.nodeValue)) continue;
          range.selectNodeContents(node);
          const rect = range.getBoundingClientRect();
          if (rect.width <= 0 || rect.height <= 0 || isHidden(parent)) continue;
          const block = blockOf(node);
          let group = groups.get(block);
          if (!group) { group = {parts: [], top: rect.top, hit: false}; groups.set(block, group); }
          group.parts.push(node.nodeValue);
          if (onScreen(rect)) group.hit = true;
        }
        const scrolled = scroller ? scroller.scrollTop : window.scrollY;
        for (const [block, group] of groups) {
          if (!group.hit) continue;
          const text = group.parts.join("").replace(/\s+/g, " ").trim();
          if (text) state.blocks.push({y: Math.round(group.top + scrolled), text, pinned: pinned(block)});
        }
        for (const anchor of document.querySelectorAll("a[href]")) {
          if (typeof anchor.href !== "string" || !/^https?:/i.test(anchor.href)) continue;
          const rect = anchor.getBoundingClientRect();
          if (rect.width <= 0 || rect.height <= 0 || !onScreen(rect) || isHidden(anchor)) continue;
          const label = anchor.getAttribute("aria-label") || anchor.innerText || anchor.title ||
            (anchor.querySelector("img[alt]") || {}).alt || "";
          state.links.push({text: String(label).replace(/\s+/g, " ").trim(), href: anchor.href});
          if (state.links.length >= \#(ProtocolLimits.webReadLinks * 4)) break;
        }
        return JSON.stringify(state);
        """#

    /// Moves the scroller by `by` points of the page, or puts it at `to`. A scroll event follows
    /// as it does for any scroll; nothing else is dispatched.
    static let scroll =
      findScroller + #"""
        const target = findScroller() || window;
        if (to !== null) target.scrollTo({top: to, behavior: "auto"});
        else target.scrollBy({top: by, behavior: "auto"});
        return true;
        """#
  }
#endif
