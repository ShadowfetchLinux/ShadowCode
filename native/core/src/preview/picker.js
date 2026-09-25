// ShadowCode preview picker. The preview proxy adds this script to HTML pages
// it serves to ShadowCode's preview frame (see native/core/src/preview.rs).
//
// It talks to one window only: the frame's parent, at the app origin the
// proxy was opened for. Messages it sends use that explicit target origin;
// messages it receives must come from window.parent with that origin. It
// reports the page address, console errors and, while picking is on, the
// element the user clicks. It never runs in a top-level window.
(function () {
  "use strict";
  var APP_ORIGIN = /*APP_ORIGIN*/"";
  if (!APP_ORIGIN || window.parent === window || window.__shadowcodePreview) return;
  window.__shadowcodePreview = true;

  var SOURCE = "shadowcode-preview";
  var VERSION = 1;
  var MAX_CONSOLE = 100;
  var MAX_TEXT = 2000;
  var MAX_HTML = 4000;

  function post(type, payload) {
    var message = { source: SOURCE, version: VERSION, type: type };
    for (var key in payload) message[key] = payload[key];
    try {
      window.parent.postMessage(message, APP_ORIGIN);
    } catch (e) {
      /* the parent navigated away */
    }
  }
  function clip(text, limit) {
    text = String(text == null ? "" : text);
    return text.length > limit ? text.slice(0, limit) + "…" : text;
  }

  // --- console errors -------------------------------------------------------
  var pending = [];
  var flushTimer = 0;
  var sent = 0;
  function describe(value) {
    if (value instanceof Error) return value.stack || value.name + ": " + value.message;
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value);
    } catch (e) {
      return String(value);
    }
  }
  function record(level, message, where) {
    if (sent >= MAX_CONSOLE) return;
    sent++;
    pending.push({
      level: level,
      message: clip(message, MAX_TEXT),
      source: where ? clip(where, 300) : "",
      url: clip(location.href, 500),
      time: Date.now(),
    });
    if (!flushTimer)
      flushTimer = setTimeout(function () {
        flushTimer = 0;
        var entries = pending;
        pending = [];
        post("console", { entries: entries });
      }, 120);
  }
  ["error", "warn"].forEach(function (level) {
    var original = console[level];
    console[level] = function () {
      try {
        record(level, Array.prototype.map.call(arguments, describe).join(" "));
      } catch (e) {
        /* never break the page's own logging */
      }
      return original.apply(this, arguments);
    };
  });
  window.addEventListener(
    "error",
    function (event) {
      var target = event.target;
      if (target && target !== window && target.tagName) {
        var url = target.src || target.href || "";
        record("error", "Failed to load " + target.tagName.toLowerCase() + (url ? " " + url : ""));
        return;
      }
      var where = event.filename ? event.filename + ":" + event.lineno + ":" + event.colno : "";
      record("error", event.error ? describe(event.error) : event.message, where);
    },
    true,
  );
  window.addEventListener("unhandledrejection", function (event) {
    record("error", "Unhandled promise rejection: " + describe(event.reason));
  });

  // --- navigation -----------------------------------------------------------
  function page() {
    return {
      url: location.href,
      title: clip(document.title, 300),
      can_go_back: history.length > 1,
    };
  }
  function navigated() {
    post("navigated", page());
  }
  ["pushState", "replaceState"].forEach(function (name) {
    var original = history[name];
    history[name] = function () {
      var result = original.apply(this, arguments);
      setTimeout(navigated, 0);
      return result;
    };
  });
  window.addEventListener("popstate", navigated);
  // Back/forward can restore the page from memory without loading it again.
  window.addEventListener("pageshow", function (event) {
    if (event.persisted) post("ready", page());
  });
  window.addEventListener("hashchange", navigated);

  // --- element picking --------------------------------------------------------
  var picking = false;
  var overlay = null;
  var box = null;
  var label = null;
  var hovered = null;

  function cssEscape(value) {
    if (window.CSS && CSS.escape) return CSS.escape(value);
    return String(value).replace(/[^a-zA-Z0-9_-]/g, function (c) {
      return "\\" + c;
    });
  }
  function unique(selector) {
    try {
      return document.querySelectorAll(selector).length === 1;
    } catch (e) {
      return false;
    }
  }
  // Class names that look generated (CSS modules, styled-components, hashes)
  // or that are state/utility variants make brittle selectors.
  function stableClass(name) {
    return (
      /^[a-zA-Z][\w-]{0,40}$/.test(name) &&
      !/\d{3,}/.test(name) &&
      !/^(css|sc|jsx|emotion|svelte)-/.test(name) &&
      !/(^|-)(hover|focus|active|open|selected|is-|has-)/.test(name) &&
      !/[A-Za-z0-9]{6,}_[A-Za-z0-9]{4,}$/.test(name)
    );
  }
  var TEST_ATTRS = ["data-testid", "data-test", "data-cy", "data-qa", "name", "aria-label", "title", "placeholder", "alt"];
  function selectorFor(el) {
    if (el.id && unique("#" + cssEscape(el.id))) return "#" + cssEscape(el.id);
    var tag = el.tagName.toLowerCase();
    for (var i = 0; i < TEST_ATTRS.length; i++) {
      var value = el.getAttribute(TEST_ATTRS[i]);
      if (value && value.length <= 80) {
        var candidate = tag + "[" + TEST_ATTRS[i] + '="' + value.replace(/["\\]/g, "\\$&") + '"]';
        if (unique(candidate)) return candidate;
      }
    }
    var parts = [];
    var node = el;
    for (var depth = 0; node && node.nodeType === 1 && depth < 8; depth++) {
      var name = node.tagName.toLowerCase();
      if (name === "html") break;
      if (node !== el && node.id && unique("#" + cssEscape(node.id))) {
        parts.unshift("#" + cssEscape(node.id));
        break;
      }
      var part = name;
      var classes = Array.prototype.filter.call(node.classList || [], stableClass).slice(0, 2);
      if (classes.length) part += "." + classes.map(cssEscape).join(".");
      var parent = node.parentElement;
      var tail = parts.length ? " > " + parts.join(" > ") : "";
      // Position only when the tag and classes do not already single it out.
      if (parent && !unique(part + tail)) {
        var same = Array.prototype.filter.call(parent.children, function (child) {
          return child.tagName === node.tagName;
        });
        if (same.length > 1) part += ":nth-of-type(" + (same.indexOf(node) + 1) + ")";
      }
      parts.unshift(part);
      var selector = parts.join(" > ");
      if (unique(selector)) return selector;
      node = parent;
    }
    return parts.join(" > ");
  }
  var IMPLICIT_ROLES = {
    a: "link",
    button: "button",
    select: "combobox",
    textarea: "textbox",
    img: "img",
    nav: "navigation",
    main: "main",
    header: "banner",
    footer: "contentinfo",
    aside: "complementary",
    form: "form",
    table: "table",
    ul: "list",
    ol: "list",
    li: "listitem",
    dialog: "dialog",
    h1: "heading",
    h2: "heading",
    h3: "heading",
    h4: "heading",
    h5: "heading",
    h6: "heading",
  };
  function roleOf(el) {
    var explicit = el.getAttribute("role");
    if (explicit) return explicit;
    var tag = el.tagName.toLowerCase();
    if (tag === "a" && !el.hasAttribute("href")) return "";
    if (tag === "input") {
      var type = (el.getAttribute("type") || "text").toLowerCase();
      if (type === "checkbox" || type === "radio") return type;
      if (type === "button" || type === "submit" || type === "reset") return "button";
      if (type === "range") return "slider";
      return "textbox";
    }
    return IMPLICIT_ROLES[tag] || "";
  }
  function nameOf(el) {
    var labelled = el.getAttribute("aria-labelledby");
    if (labelled) {
      var text = labelled
        .split(/\s+/)
        .map(function (id) {
          var ref = document.getElementById(id);
          return ref ? ref.textContent : "";
        })
        .join(" ")
        .trim();
      if (text) return text;
    }
    return (
      el.getAttribute("aria-label") ||
      el.getAttribute("alt") ||
      el.getAttribute("title") ||
      el.getAttribute("placeholder") ||
      ""
    );
  }
  var STYLES = [
    "display",
    "position",
    "box-sizing",
    "width",
    "height",
    "margin",
    "padding",
    "color",
    "background-color",
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "text-align",
    "border",
    "border-radius",
    "box-shadow",
    "flex-direction",
    "justify-content",
    "align-items",
    "gap",
    "grid-template-columns",
    "overflow",
    "opacity",
    "visibility",
    "z-index",
    "cursor",
  ];
  var SKIP_VALUES = { none: 1, normal: 1, auto: 1, visible: 1, "0px": 1, "rgba(0, 0, 0, 0)": 1, static: 1 };
  function stylesOf(el) {
    var computed = getComputedStyle(el);
    var out = {};
    STYLES.forEach(function (name) {
      var value = computed.getPropertyValue(name);
      if (value && !(name !== "display" && SKIP_VALUES[value])) out[name] = clip(value, 200);
    });
    return out;
  }
  function attributesOf(el) {
    var out = {};
    var count = 0;
    Array.prototype.forEach.call(el.attributes, function (attr) {
      if (count >= 16 || /^on/i.test(attr.name) || attr.name === "style") return;
      out[attr.name] = clip(attr.value, 300);
      count++;
    });
    return out;
  }
  function outerHtmlOf(el) {
    var html = el.outerHTML || "";
    if (html.length <= MAX_HTML) return html;
    var open = html.slice(0, html.indexOf(">") + 1);
    return clip(open + (el.innerHTML || ""), MAX_HTML) + "</" + el.tagName.toLowerCase() + ">";
  }
  function capture(el) {
    var rect = el.getBoundingClientRect();
    var ancestors = [];
    for (var node = el.parentElement; node && ancestors.length < 4 && node !== document.body; node = node.parentElement) {
      ancestors.push(node.tagName.toLowerCase() + (node.id ? "#" + node.id : "") + (node.classList.length ? "." + Array.prototype.slice.call(node.classList, 0, 2).join(".") : ""));
    }
    return {
      selector: selectorFor(el),
      tag: el.tagName.toLowerCase(),
      role: roleOf(el),
      name: clip(nameOf(el), 300),
      text: clip((el.innerText || el.textContent || "").replace(/\s+/g, " ").trim(), 300),
      attributes: attributesOf(el),
      outer_html: outerHtmlOf(el),
      styles: stylesOf(el),
      box: {
        x: Math.round(rect.left),
        y: Math.round(rect.top),
        width: Math.round(rect.width),
        height: Math.round(rect.height),
      },
      ancestors: ancestors,
      url: location.href,
      title: clip(document.title, 300),
      viewport: { width: window.innerWidth, height: window.innerHeight, dpr: window.devicePixelRatio || 1 },
    };
  }

  function ensureOverlay() {
    if (overlay) return;
    overlay = document.createElement("div");
    overlay.setAttribute("data-shadowcode-preview", "");
    overlay.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483647;";
    box = document.createElement("div");
    box.style.cssText =
      "position:fixed;border:2px solid #6d5dfc;background:rgba(109,93,252,.14);border-radius:3px;display:none;box-sizing:border-box;";
    label = document.createElement("div");
    label.style.cssText =
      "position:fixed;display:none;font:12px/1.4 ui-monospace,monospace;background:#1f1d2b;color:#fff;padding:2px 6px;border-radius:4px;white-space:nowrap;max-width:60vw;overflow:hidden;text-overflow:ellipsis;";
    overlay.appendChild(box);
    overlay.appendChild(label);
    (document.body || document.documentElement).appendChild(overlay);
  }
  function highlight(el) {
    hovered = el;
    if (!el) {
      box.style.display = label.style.display = "none";
      return;
    }
    var rect = el.getBoundingClientRect();
    box.style.display = "block";
    box.style.left = rect.left + "px";
    box.style.top = rect.top + "px";
    box.style.width = rect.width + "px";
    box.style.height = rect.height + "px";
    var classes = Array.prototype.slice.call(el.classList || [], 0, 2).join(".");
    label.textContent =
      el.tagName.toLowerCase() + (el.id ? "#" + el.id : "") + (classes ? "." + classes : "") + "  " + Math.round(rect.width) + "×" + Math.round(rect.height);
    label.style.display = "block";
    label.style.left = Math.max(0, rect.left) + "px";
    label.style.top = (rect.top > 24 ? rect.top - 22 : rect.bottom + 4) + "px";
  }
  function elementAt(event) {
    var el = event.target;
    if (!el || el.nodeType !== 1 || (overlay && overlay.contains(el))) return null;
    return el === document.documentElement ? null : el;
  }
  function onMove(event) {
    if (picking) highlight(elementAt(event));
  }
  function swallow(event) {
    if (!picking) return;
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation();
  }
  function onClick(event) {
    if (!picking) return;
    swallow(event);
    var el = elementAt(event) || hovered;
    if (!el) return;
    setPicking(false);
    post("picked", { element: capture(el) });
  }
  function onKey(event) {
    if (picking && event.key === "Escape") {
      swallow(event);
      setPicking(false);
      post("pick-cancelled", {});
    }
  }
  function setPicking(on) {
    picking = Boolean(on);
    if (picking) {
      ensureOverlay();
      document.documentElement.style.cursor = "crosshair";
    } else {
      document.documentElement.style.cursor = "";
      if (overlay) highlight(null);
    }
  }
  document.addEventListener("mousemove", onMove, true);
  ["mousedown", "mouseup", "pointerdown", "pointerup", "dblclick", "contextmenu", "submit"].forEach(function (type) {
    document.addEventListener(type, swallow, true);
  });
  document.addEventListener("click", onClick, true);
  document.addEventListener("keydown", onKey, true);

  // --- commands from the app ------------------------------------------------
  window.addEventListener("message", function (event) {
    if (event.source !== window.parent || event.origin !== APP_ORIGIN) return;
    var data = event.data;
    if (!data || typeof data !== "object" || data.source !== "shadowcode-app") return;
    switch (data.type) {
      case "hello":
        post("ready", page());
        break;
      case "pick":
        setPicking(Boolean(data.on));
        break;
      case "back":
        history.back();
        break;
      case "forward":
        history.forward();
        break;
      case "reload":
        location.reload();
        break;
    }
  });
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", function () {
    post("ready", page());
  });
  else post("ready", page());
})();
