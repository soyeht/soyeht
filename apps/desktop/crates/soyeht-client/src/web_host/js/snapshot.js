// Reads the page for an agent: its controls, headings and text, in document order, each with the
// rectangle it takes in the viewport. It runs in an isolated script world, so the page cannot
// see this function, what it keeps, or that it ran; it changes nothing in the page, adds no node
// and sends no event.
(function (budget, generation) {
  "use strict";
  const SKIPPED = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD", "TITLE", "META", "LINK"]);
  const WIDGETS = "a,button,input,select,textarea,summary,h1,h2,h3,h4,h5,h6,label," +
    "[role=button],[role=link],[role=checkbox],[role=textbox],[role=tab],[role=menuitem]," +
    "[role=option],[role=switch],[role=combobox],[role=radio],[role=heading],[role=searchbox]";
  const ROLES = {
    A: "link", BUTTON: "button", SELECT: "combobox", TEXTAREA: "textbox", SUMMARY: "button",
    H1: "heading", H2: "heading", H3: "heading", H4: "heading", H5: "heading", H6: "heading",
    LABEL: "label"
  };
  const clamp = (value, low, high) => Math.max(low, Math.min(high, Math.round(value)));
  const visible = (element, rect) => {
    if (rect.width <= 0 || rect.height <= 0) return false;
    const style = getComputedStyle(element);
    return style.display !== "none" && style.visibility !== "hidden";
  };
  const roleOf = (element) => element.getAttribute("role") ||
    (element.tagName === "INPUT" ? (element.type === "checkbox" ? "checkbox" : "textbox") :
      ROLES[element.tagName] || element.tagName.toLowerCase());
  const squeeze = (text) => String(text || "").replace(/\s+/g, " ").trim();
  const nameOf = (element) => squeeze(element.getAttribute("aria-label") || element.alt ||
    element.title || element.placeholder || element.innerText).slice(0, budget.labelChars);
  const ownText = (element) => {
    let text = "";
    for (const child of element.childNodes) {
      if (child.nodeType === 3) text += " " + child.nodeValue;
    }
    return squeeze(text).slice(0, budget.labelChars);
  };
  const stateOf = (element) => [
    element.disabled ? "disabled" : "",
    element.checked === true ? "checked" : "",
    element.selected === true ? "selected" : "",
    element.getAttribute("aria-expanded") ? "expanded=" + element.getAttribute("aria-expanded") : ""
  ].filter(Boolean).join(" ").slice(0, budget.labelChars);

  const refs = new Map(); // the controls and headings, which cover what is inside them
  const byElement = new Map(); // every node's element, for the parent of the next
  const references = new Map();
  const nodes = [];
  let truncated = false;
  const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_ELEMENT);
  for (let element = walker.currentNode; element; element = walker.nextNode()) {
    if (SKIPPED.has(element.tagName)) continue;
    // A control or a heading is named by everything inside it and covers it. Any other element
    // (a region, a list, a paragraph) is named by its label or its own text only, and covers
    // nothing, so the text of a whole page is not read as the name of its main region.
    const interactive = element.matches(WIDGETS);
    const fields = ["INPUT", "SELECT", "TEXTAREA"].includes(element.tagName);
    const label = squeeze(element.getAttribute("aria-label")).slice(0, budget.labelChars);
    const name = interactive ? nameOf(element) : (label || ownText(element));
    if (!name && !(interactive && fields)) continue;
    let covered = false;
    for (let up = element.parentElement; up && !covered; up = up.parentElement) covered = refs.has(up);
    // The text of a link or a heading is its name; it is not read a second time as text.
    if (!interactive && covered) continue;
    const rect = element.getBoundingClientRect();
    if (!visible(element, rect)) continue;
    if (nodes.length >= budget.maxNodes) { truncated = true; break; }
    const ref = "e" + (nodes.length + 1);
    let parent = element.parentElement;
    while (parent && !byElement.has(parent)) parent = parent.parentElement;
    byElement.set(element, ref);
    references.set(ref, element);
    if (interactive) refs.set(element, ref);
    const role = interactive ? roleOf(element) : (element.getAttribute("role") || (label ? "group" : "text"));
    nodes.push({
      ref, role: role.slice(0, budget.tokenChars), name,
      state: stateOf(element),
      value: element.tagName === "A" && element.href ? String(element.href).slice(0, budget.fieldChars) :
        ("value" in element ? String(element.value).slice(0, budget.fieldChars) : null),
      rect: {
        x: clamp(rect.x, -2147483648, 2147483647), y: clamp(rect.y, -2147483648, 2147483647),
        width: clamp(rect.width, 0, budget.extentMax), height: clamp(rect.height, 0, budget.extentMax)
      },
      parentRef: parent ? byElement.get(parent) : null
    });
  }
  globalThis.__soyehtReferences = references;
  globalThis.__soyehtGeneration = generation;
  return JSON.stringify({ nodes, truncated });
})
