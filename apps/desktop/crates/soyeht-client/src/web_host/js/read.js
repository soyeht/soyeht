// Reads one screen of the page for `web.read`: where the page is scrolled, what scrolls, what is
// pinned to the edges, and the visible text and links. It runs in an isolated script world, so the
// page cannot see this function or what it keeps, and it only reads: no event, no node, no global
// of the page's own is touched.
(function (collect, linkCap) {
  "use strict";
  const root = document.body || document.documentElement;
  const doc = document.scrollingElement || document.documentElement;
  const view = { width: window.innerWidth, height: window.innerHeight };
  const onScreen = (rect) => rect.bottom > 0 && rect.top < view.height &&
    rect.right > 0 && rect.left < view.width;

  // What scrolls: the document, or, when the document does not, the large element that does (an
  // application shell keeps its feed in one).
  const scrolls = (element) => element.scrollHeight > element.clientHeight + 4;
  let scroller = null;
  if (!scrolls(doc)) {
    let best = 0;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
    let seen = 0;
    for (let element = walker.nextNode(); element && seen < 20000; element = walker.nextNode(), seen++) {
      if (!scrolls(element)) continue;
      const overflow = getComputedStyle(element).overflowY;
      if (overflow !== "auto" && overflow !== "scroll" && overflow !== "overlay") continue;
      const rect = element.getBoundingClientRect();
      const area = Math.max(0, Math.min(rect.bottom, view.height) - Math.max(rect.top, 0)) *
        Math.max(0, Math.min(rect.right, view.width) - Math.max(rect.left, 0));
      if (area < view.width * view.height * 0.3) continue;
      const weight = area * element.scrollHeight;
      if (weight > best) { best = weight; scroller = element; }
    }
  }
  const area = scroller || doc;
  const rect = scroller ? scroller.getBoundingClientRect() : { top: 0, bottom: view.height, left: 0, right: view.width };
  const state = {
    inner: scroller !== null,
    scrollTop: area.scrollTop,
    scrollHeight: area.scrollHeight,
    viewWidth: view.width, viewHeight: view.height,
    top: Math.max(0, rect.top), bottom: Math.min(view.height, rect.bottom),
    left: Math.max(0, rect.left), right: Math.min(view.width, rect.right),
    fixedTop: 0, fixedBottom: 0, blocks: [], links: []
  };

  // Bands pinned to the top or the bottom of the view, which every screen shows again.
  const band = (element) => {
    for (let current = element; current && current !== root; current = current.parentElement) {
      const position = getComputedStyle(current).position;
      if (position === "fixed" || position === "sticky") return current;
    }
    return null;
  };
  const pins = new Map();
  const pinned = (element) => {
    if (!pins.has(element)) pins.set(element, band(element) !== null);
    return pins.get(element);
  };
  const edges = document.querySelectorAll("header, nav, footer, [role=banner], [role=navigation], div, aside");
  let scanned = 0;
  for (const element of edges) {
    if (scanned++ > 4000) break;
    const position = getComputedStyle(element).position;
    if (position !== "fixed" && position !== "sticky") continue;
    const box = element.getBoundingClientRect();
    if (box.width < view.width * 0.5 || box.height <= 0 || box.height > view.height * 0.6) continue;
    if (box.top <= 1 && box.bottom > 0) state.fixedTop = Math.max(state.fixedTop, Math.ceil(box.bottom));
    else if (box.bottom >= view.height - 16 && box.top < view.height) {
      state.fixedBottom = Math.max(state.fixedBottom, Math.ceil(view.height - box.top));
    }
  }
  if (!collect) return JSON.stringify(state);

  const skipped = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD", "TITLE"]);
  const displays = new Map();
  const hidden = new Map();
  const displayOf = (element) => {
    if (!displays.has(element)) displays.set(element, getComputedStyle(element).display);
    return displays.get(element);
  };
  const blockOf = (node) => {
    for (let element = node.parentElement; element && element !== root; element = element.parentElement) {
      const display = displayOf(element);
      if (!display.startsWith("inline") && display !== "contents") return element;
    }
    return root;
  };
  const isHidden = (element) => {
    if (!hidden.has(element)) hidden.set(element, getComputedStyle(element).visibility !== "visible");
    return hidden.get(element);
  };
  const within = (box) => onScreen(box) && box.bottom > state.top && box.top < state.bottom;
  const groups = new Map();
  const range = document.createRange();
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let visited = 0;
  for (let node = walker.nextNode(); node && visited < 40000; node = walker.nextNode(), visited++) {
    const parent = node.parentElement;
    if (!parent || skipped.has(parent.tagName) || !/\S/.test(node.nodeValue)) continue;
    range.selectNodeContents(node);
    const box = range.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0 || isHidden(parent)) continue;
    const block = blockOf(node);
    let group = groups.get(block);
    if (!group) { group = { parts: [], top: box.top, hit: false }; groups.set(block, group); }
    group.parts.push(node.nodeValue);
    if (within(box)) group.hit = true;
  }
  state.capped = visited >= 40000;
  for (const [block, group] of groups) {
    if (!group.hit) continue;
    const text = group.parts.join("").replace(/\s+/g, " ").trim();
    if (text) state.blocks.push({ y: Math.round(group.top + area.scrollTop), text, pinned: pinned(block) });
  }
  for (const anchor of document.querySelectorAll("a[href]")) {
    if (typeof anchor.href !== "string" || !/^https?:/i.test(anchor.href)) continue;
    const box = anchor.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0 || !within(box) || isHidden(anchor)) continue;
    const label = anchor.getAttribute("aria-label") || anchor.innerText || anchor.title ||
      (anchor.querySelector("img[alt]") || {}).alt || "";
    state.links.push({ text: String(label).replace(/\s+/g, " ").trim(), href: anchor.href });
    if (state.links.length >= linkCap) break;
  }
  return JSON.stringify(state);
})
