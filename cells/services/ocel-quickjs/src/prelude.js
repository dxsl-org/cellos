/* SPDX-License-Identifier: MIT
 * Bounded document-backed DOM and browser event loop. Network and pixel/CSS
 * data come only from authenticated native commands, never from JS guesses.
 */
(function () {
    "use strict";
    const globalEval = (0, eval), own = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
    let nodes = new Map(), records = new WeakMap(), nextId = 0;
    let mutations = [], logs = [], title = "", mutationBytes = 0, logBytes = 0;
    let requests = [], requestBytes = 0, nextRequest = 1, pending = new Map();
    let environment = null, clock = 0, width = 0, height = 0, scrollY = 0, cookie = "";
    let layouts = new Map(), readyState = "loading", currentScript = null, parsingComplete = false, loaded = false;
    const timers = new Map(), eventTargets = new WeakMap();
    let nextTimer = 1;
    const MAX_MUTATIONS = 1024, MAX_MUTATION_BYTES = 131072, MAX_LOG_LINES = 64, MAX_LOG_BYTES = 8192;
    const MAX_REQUESTS = 64, MAX_REQUEST_BYTES = 131072, MAX_PENDING = 64, MAX_TIMERS = 256, MAX_CALLBACKS = 64;
    const str = value => String(value).toWellFormed(), enc = value => encodeURIComponent(str(value));
    function encodedLength(text, remaining) {
        if (text.length > remaining) throw new RangeError("JavaScript output byte limit exceeded");
        let size = 0;
        for (let i = 0; i < text.length; ++i) {
            const c = text.charCodeAt(i);
            if (c < 128) size += /[A-Za-z0-9_.!~*'()-]/.test(text[i]) ? 1 : 3;
            else if (c < 2048) size += 6;
            else if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length && text.charCodeAt(i + 1) >= 0xdc00 && text.charCodeAt(i + 1) <= 0xdfff) { size += 12; ++i; }
            else size += 9;
            if (size > remaining) throw new RangeError("JavaScript output byte limit exceeded");
        }
        return size;
    }
    function wire(kind, fields, used, maximum) {
        const strings = fields.map(str);
        let size = kind.length + strings.length + 1;
        for (const text of strings) size += encodedLength(text, maximum - used - size);
        return { text: [kind, ...strings.map(enc)].join("\t"), size };
    }
    function emit(kind, ...fields) {
        if (mutations.length >= MAX_MUTATIONS) throw new RangeError("JavaScript mutation count limit exceeded");
        const row = wire(kind, fields, mutationBytes, MAX_MUTATION_BYTES);
        mutations.push(row.text); mutationBytes += row.size;
    }
    function request(kind, ...fields) {
        if (!environment) throw new Error("Native browser environment is not configured");
        if (requests.length >= MAX_REQUESTS) throw new RangeError("Browser request count limit exceeded");
        const row = wire(kind, fields, requestBytes, MAX_REQUEST_BYTES);
        requests.push(row.text); requestBytes += row.size;
    }
    function allocateRequest() {
        if (pending.size >= MAX_PENDING || nextRequest > 0xffffffff) throw new RangeError("Browser pending request limit exceeded");
        return nextRequest++;
    }
    function log(prefix, args) {
        if (logs.length >= MAX_LOG_LINES) throw new RangeError("JavaScript log count limit exceeded");
        let text = prefix;
        for (let i = 0; i < args.length; ++i) {
            const part = String(args[i]);
            if (part.length > (MAX_LOG_BYTES - logBytes - text.length * 3 - 1) / 3) throw new RangeError("JavaScript log byte limit exceeded");
            text += (i ? " " : "") + part;
        }
        const bytes = text.length * 3 + 1;
        if (bytes > MAX_LOG_BYTES - logBytes) throw new RangeError("JavaScript log byte limit exceeded");
        logs.push(text); logBytes += bytes;
    }
    function hook(name, fn) { Object.defineProperty(globalThis, name, { value: fn }); }
    function record(node) {
        const r = records.get(node);
        if (!r || nodes.get(r.id) !== node) throw new TypeError("Node belongs to a different document");
        return r;
    }
    function connected(node) {
        for (let n = node; n; n = record(n).parent) if (record(n).type === 9) return true;
        return false;
    }
    function descendants(root, predicate) {
        const found = [], stack = record(root).children.slice().reverse();
        while (stack.length) {
            const node = stack.pop(), r = record(node);
            if (r.type === 1 && predicate(node)) found.push(node);
            for (let i = r.children.length - 1; i >= 0; --i) stack.push(r.children[i]);
        }
        return collection(found);
    }
    function collection(items) {
        Object.defineProperty(items, "item", { value: i => items[i] || null });
        Object.defineProperty(items, "namedItem", { value: name => items.find(n => n.id === name || n.getAttribute("name") === name) || null });
        return items;
    }
    class Event {
        constructor(type, options = {}) {
            this.type = String(type); this.bubbles = !!options.bubbles; this.cancelable = !!options.cancelable;
            this.target = null; this.currentTarget = null; this.eventPhase = 0; this.defaultPrevented = false;
            this.timeStamp = clock; this.isTrusted = false; this._stopped = false; this._immediate = false;
        }
        preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
        stopPropagation() { this._stopped = true; }
        stopImmediatePropagation() { this._stopped = true; this._immediate = true; }
        initEvent(type, bubbles, cancelable) { this.type = String(type); this.bubbles = !!bubbles; this.cancelable = !!cancelable; }
    }
    class CustomEvent extends Event {
        constructor(type, options = {}) { super(type, options); this.detail = options.detail === undefined ? null : options.detail; }
        initCustomEvent(type, bubbles, cancelable, detail) { this.initEvent(type, bubbles, cancelable); this.detail = detail; }
    }
    function listeners(target) { let list = eventTargets.get(target); if (!list) { list = []; eventTargets.set(target, list); } return list; }
    function addEventListener(kind, handler, options = false) {
        if (handler == null) return;
        if (typeof handler !== "function" && typeof handler.handleEvent !== "function") throw new TypeError("Invalid event listener");
        const capture = typeof options === "boolean" ? options : !!options.capture, list = listeners(this);
        kind = String(kind);
        if (!list.some(l => l.kind === kind && l.handler === handler && l.capture === capture)) {
            if (list.length >= 1024) throw new RangeError("Event listener limit exceeded");
            list.push({ kind, handler, capture, once: !!(options && options.once) });
        }
    }
    function removeEventListener(kind, handler, options = false) {
        const capture = typeof options === "boolean" ? options : !!options.capture;
        eventTargets.set(this, listeners(this).filter(l => l.kind !== String(kind) || l.handler !== handler || l.capture !== capture));
    }
    function invoke(target, event, capture) {
        event.currentTarget = target;
        for (const l of listeners(target).slice()) {
            if (l.kind !== event.type || l.capture !== capture || !listeners(target).includes(l)) continue;
            if (l.once) target.removeEventListener(l.kind, l.handler, l.capture);
            if (typeof l.handler === "function") l.handler.call(target, event); else l.handler.handleEvent(event);
            if (event._immediate) break;
        }
        if (!capture && !event._immediate) {
            let handler = target["on" + event.type];
            if (handler == null && records.has(target)) {
                const source = record(target).attrs["on" + event.type];
                if (source) handler = new Function("event", source);
            }
            if (typeof handler === "function" && handler.call(target, event) === false) event.preventDefault();
        }
    }
    function dispatchEvent(event) {
        if (!(event instanceof Event)) throw new TypeError("dispatchEvent requires Event");
        if (!event.type || event.eventPhase) throw new Error("InvalidStateError");
        event.target = this;
        const path = [this];
        if (records.has(this)) {
            for (let p = record(this).parent; p; p = record(p).parent) path.push(p);
            if (record(path[path.length - 1]).type === 9) path.push(globalThis);
        }
        try {
            event.eventPhase = 1;
            for (let i = path.length - 1; i > 0 && !event._stopped; --i) invoke(path[i], event, true);
            if (!event._stopped) { event.eventPhase = 2; invoke(this, event, true); if (!event._immediate) invoke(this, event, false); }
            if (event.bubbles) { event.eventPhase = 3; for (let i = 1; i < path.length && !event._stopped; ++i) invoke(path[i], event, false); }
        } finally { event.currentTarget = null; event.eventPhase = 0; }
        return !event.defaultPrevented;
    }
    function fire(target, type, options) { return target.dispatchEvent(new Event(type, options)); }
    function eventMethods(target) { Object.assign(target, { addEventListener, removeEventListener, dispatchEvent }); }
    class EventTarget { constructor() {} }
    eventMethods(EventTarget.prototype);
    eventMethods(globalThis);

    function detach(node) {
        const r = record(node);
        if (r.parent) { const children = record(r.parent).children; children.splice(children.indexOf(node), 1); r.parent = null; }
    }
    function insert(parent, child, reference, notify = false) {
        const p = record(parent), c = record(child);
        if (![1, 9, 11].includes(p.type) || c.type === 9) throw new Error("HierarchyRequestError");
        if (reference !== null && record(reference).parent !== parent) throw new Error("NotFoundError");
        if (child === reference) return child;
        for (let cursor = parent; cursor; cursor = record(cursor).parent) if (cursor === child) throw new Error("HierarchyRequestError");
        if (c.type === 11) { for (const node of c.children.slice()) insert(parent, node, reference, notify); return child; }
        if (notify) { if (reference) emit("I", p.id, c.id, record(reference).id); else emit("P", p.id, c.id); }
        detach(child);
        p.children.splice(reference ? p.children.indexOf(reference) : p.children.length, 0, child); c.parent = parent;
        if (notify) activateTree(child);
        return child;
    }
    function append(parent, child, notify = false) { return insert(parent, child, null, notify); }
    function textOf(node) {
        const r = record(node);
        if (r.type === 9) return null;
        if (r.type === 3 || r.type === 8) return r.text;
        let text = "", stack = r.children.slice().reverse();
        while (stack.length) { const current = record(stack.pop()); if (current.type === 3) text += current.text; for (let i = current.children.length - 1; i >= 0; --i) stack.push(current.children[i]); }
        return text;
    }
    function makeNode(id, type, tag, text, attrs) {
        const node = Object.create(Node.prototype);
        records.set(node, { id, type, tag, text, attrs: Object.assign(Object.create(null), Object.fromEntries(attrs)), parent: null, children: [], resource: null, scriptStarted: false, inertScript: false });
        nodes.set(id, node); globalThis["node_" + id] = node; return node;
    }
    function create(type, tag, text, notify = true) {
        const id = nextId;
        if (notify) emit("C", id, type, tag, text);
        ++nextId;
        return makeNode(id, type, tag, text, []);
    }
    function Node() { throw new TypeError("Use document.createElement/createTextNode/createDocumentFragment"); }
    eventMethods(Node.prototype);
    Object.assign(Node, { ELEMENT_NODE: 1, TEXT_NODE: 3, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_FRAGMENT_NODE: 11 });
    Object.assign(Node.prototype, Node);
    Object.defineProperties(Node.prototype, {
        nodeType: { get() { return record(this).type; } },
        nodeName: { get() { const r = record(this); return r.type === 1 ? r.tag.toUpperCase() : r.type === 3 ? "#text" : r.type === 8 ? "#comment" : r.type === 11 ? "#document-fragment" : "#document"; } },
        tagName: { get() { return record(this).type === 1 ? record(this).tag.toUpperCase() : undefined; } },
        localName: { get() { return record(this).type === 1 ? record(this).tag : null; } },
        namespaceURI: { get() { return record(this).type === 1 ? "http://www.w3.org/1999/xhtml" : null; } },
        ownerDocument: { get() { return record(this).type === 9 ? null : globalThis.document; } },
        parentNode: { get() { return record(this).parent; } },
        parentElement: { get() { const p = this.parentNode; return p && p.nodeType === 1 ? p : null; } },
        isConnected: { get() { return connected(this); } },
        firstChild: { get() { return record(this).children[0] || null; } },
        lastChild: { get() { const c = record(this).children; return c[c.length - 1] || null; } },
        childNodes: { get() { return collection(record(this).children.slice()); } },
        children: { get() { return collection(record(this).children.filter(n => record(n).type === 1)); } },
        childElementCount: { get() { return this.children.length; } },
        firstElementChild: { get() { return this.children[0] || null; } },
        lastElementChild: { get() { const c = this.children; return c[c.length - 1] || null; } },
        nextSibling: { get() { const p = this.parentNode; return p ? record(p).children[record(p).children.indexOf(this) + 1] || null : null; } },
        previousSibling: { get() { const p = this.parentNode; return p ? record(p).children[record(p).children.indexOf(this) - 1] || null : null; } },
        nextElementSibling: { get() { let n = this.nextSibling; while (n && n.nodeType !== 1) n = n.nextSibling; return n; } },
        previousElementSibling: { get() { let n = this.previousSibling; while (n && n.nodeType !== 1) n = n.previousSibling; return n; } },
        nodeValue: { get() { return [3, 8].includes(this.nodeType) ? record(this).text : null; }, set(v) { if ([3, 8].includes(this.nodeType)) this.textContent = v; } },
        data: { get() { return record(this).text; }, set(v) { if (![3, 8].includes(this.nodeType)) throw new TypeError("Not character data"); this.textContent = v; } },
        textContent: {
            get() { return textOf(this); },
            set(value) {
                const r = record(this), text = value == null ? "" : str(value);
                if (r.type === 9) return;
                emit("X", r.id, text);
                if (r.type === 3 || r.type === 8) r.text = text;
                else { const removed = r.children; for (const child of removed) record(child).parent = null; r.children = []; if (text) append(this, create(3, "", text, false)); for (const child of removed) cancelTree(child); }
            }
        },
        innerText: { get() { return this.textContent; }, set(v) { this.textContent = v; } },
        text: { get() { return this.textContent; }, set(v) { this.textContent = v; } },
        id: { get() { return this.getAttribute("id") || ""; }, set(v) { this.setAttribute("id", v); } },
        className: { get() { return this.getAttribute("class") || ""; }, set(v) { this.setAttribute("class", v); } },
        attributes: { get() { return collection(Object.entries(record(this).attrs).map(([name, value]) => ({ name, nodeName: name, value, nodeValue: value, specified: true }))); } },
        innerHTML: { get() { return record(this).children.map(serialize).join(""); }, set(v) { const fragment = parseFragment(str(v)); for (const n of this.childNodes) this.removeChild(n); this.appendChild(fragment); } },
        outerHTML: { get() { return serialize(this); }, set(v) { const p = this.parentNode; if (!p) return; const f = parseFragment(str(v)); p.insertBefore(f, this); p.removeChild(this); } },
        classList: { get() { const r = record(this); return r.classList || (r.classList = tokenList(this)); } },
        style: { get() { const r = record(this); return r.style || (r.style = styleDeclaration(this)); }, set(v) { this.setAttribute("style", v); } },
        dataset: { get() { const n = this; return new Proxy({}, { get(_, k) { return typeof k === "string" ? n.getAttribute("data-" + dash(k)) || undefined : undefined; }, set(_, k, v) { n.setAttribute("data-" + dash(k), v); return true; } }); } },
    });
    Node.prototype.appendChild = function (child) { return append(this, child, true); };
    Node.prototype.insertBefore = function (child, reference) { return insert(this, child, reference == null ? null : reference, true); };
    Node.prototype.replaceChild = function (child, old) { if (old.parentNode !== this) throw new Error("NotFoundError"); if (old !== child) { this.insertBefore(child, old); this.removeChild(old); } return old; };
    Node.prototype.removeChild = function (child) { if (record(child).parent !== this) throw new Error("NotFoundError"); emit("D", record(this).id, record(child).id); detach(child); cancelTree(child); return child; };
    Node.prototype.remove = function () { if (this.parentNode) this.parentNode.removeChild(this); };
    Node.prototype.hasChildNodes = function () { return record(this).children.length !== 0; };
    Node.prototype.append = function (...items) { for (const n of items) this.appendChild(records.has(n) ? n : document.createTextNode(n)); };
    Node.prototype.prepend = function (...items) { const ref = this.firstChild; for (const n of items) this.insertBefore(records.has(n) ? n : document.createTextNode(n), ref); };
    Node.prototype.replaceWith = function (...items) { const p = this.parentNode; if (!p) return; for (const n of items) p.insertBefore(records.has(n) ? n : document.createTextNode(n), this); this.remove(); };
    Node.prototype.cloneNode = function (deep = false) {
        const r = record(this); if (r.type === 9) throw new Error("NotSupportedError");
        const copy = create(r.type, r.tag, r.text);
        for (const [name, value] of Object.entries(r.attrs)) copy.setAttribute(name, value);
        const c = record(copy); c.scriptStarted = r.scriptStarted; c.inertScript = r.inertScript;
        for (const name of ["valueState", "checkedState", "selectedState"]) if (own(r, name)) c[name] = r[name];
        if (deep) for (const child of r.children) copy.appendChild(child.cloneNode(true));
        return copy;
    };
    Node.prototype.contains = function (other) { if (!records.has(other)) return false; for (let n = other; n; n = n.parentNode) if (n === this) return true; return false; };
    Node.prototype.compareDocumentPosition = function (other) {
        record(other); if (other === this) return 0;
        if (this.contains(other)) return 20; if (other.contains(this)) return 10;
        const a = [], b = []; for (let n = this; n; n = n.parentNode) a.unshift(n); for (let n = other; n; n = n.parentNode) b.unshift(n);
        if (a[0] !== b[0]) return 1 | 32 | (record(this).id < record(other).id ? 4 : 2);
        let i = 0; while (a[i] === b[i]) ++i;
        return record(a[i - 1]).children.indexOf(a[i]) < record(a[i - 1]).children.indexOf(b[i]) ? 4 : 2;
    };
    Node.prototype.setAttribute = function (key, value) {
        const r = record(this); if (r.type !== 1) throw new TypeError("Attributes require an element");
        key = str(key).toLowerCase(); value = str(value);
        if (!key || /[\s\0"'>/=]/.test(key)) throw new Error("InvalidCharacterError");
        emit("A", r.id, key, value); r.attrs[key] = value;
        if (["src", "href", "rel", "type"].includes(key)) activate(this);
    };
    Node.prototype.getAttribute = function (key) { const r = record(this); key = String(key).toLowerCase(); return own(r.attrs, key) ? r.attrs[key] : null; };
    Node.prototype.getAttributeNode = function (key) { const value = this.getAttribute(key); return value === null ? null : { name: String(key), value, nodeValue: value, specified: true }; };
    Node.prototype.hasAttribute = function (key) { return this.getAttribute(key) !== null; };
    Node.prototype.removeAttribute = function (key) { const r = record(this); if (r.type !== 1) throw new TypeError("Attributes require an element"); key = String(key).toLowerCase(); emit("R", r.id, key); delete r.attrs[key]; if (["src", "href", "rel", "type"].includes(key)) activate(this); };
    Node.prototype.getElementsByTagName = function (tag) { tag = String(tag).toLowerCase(); return descendants(this, n => tag === "*" || record(n).tag === tag); };
    Node.prototype.getElementsByClassName = function (name) { const wanted = String(name).trim().split(/\s+/); return descendants(this, n => wanted.every(k => n.classList.contains(k))); };

    function tokenList(node) {
        const tokens = () => node.className.trim().split(/\s+/).filter(Boolean);
        const check = token => { token = String(token); if (!token || /\s/.test(token)) throw new Error("InvalidCharacterError"); return token; };
        const api = {
            contains(t) { return tokens().includes(check(t)); },
            add(...items) { const list = tokens(); for (const t of items.map(check)) if (!list.includes(t)) list.push(t); node.className = list.join(" "); },
            remove(...items) { const remove = items.map(check); node.className = tokens().filter(t => !remove.includes(t)).join(" "); },
            toggle(t, force) { t = check(t); const yes = force === undefined ? !this.contains(t) : !!force; if (yes) this.add(t); else this.remove(t); return yes; },
            replace(a, b) { a = check(a); b = check(b); const list = tokens(); if (!list.includes(a)) return false; node.className = [...new Set(list.map(t => t === a ? b : t))].join(" "); return true; },
            item(i) { return tokens()[i] || null; }, toString() { return node.className; },
            get length() { return tokens().length; }, get value() { return node.className; }, set value(v) { node.className = v; },
            [Symbol.iterator]() { return tokens()[Symbol.iterator](); }
        };
        return new Proxy(api, { get(o, k) { return /^\d+$/.test(String(k)) ? tokens()[Number(k)] : Reflect.get(o, k); } });
    }
    const dash = name => String(name).replace(/[A-Z]/g, c => "-" + c.toLowerCase());
    function declarations(text) {
        const out = new Map(); let start = 0, depth = 0, quote = "";
        const add = chunk => { const colon = chunk.indexOf(":"); if (colon < 1) return; const key = chunk.slice(0, colon).trim().toLowerCase(); let value = chunk.slice(colon + 1).trim(); const important = /\s*!important\s*$/i.test(value); if (important) value = value.replace(/\s*!important\s*$/i, "").trim(); if (value) out.set(key, [value, important ? "important" : ""]); };
        for (let i = 0; i <= text.length; ++i) { const c = text[i]; if (quote) { if (c === "\\") ++i; else if (c === quote) quote = ""; } else if (c === '"' || c === "'") quote = c; else if (c === "(") ++depth; else if (c === ")") --depth; else if ((!depth && c === ";") || i === text.length) { add(text.slice(start, i)); start = i + 1; } }
        return out;
    }
    function styleDeclaration(node, snapshot) {
        const values = () => snapshot ? new Map(snapshot.map(([k, v]) => [k, [v, ""]])) : declarations(node.getAttribute("style") || "");
        const write = map => node.setAttribute("style", [...map].map(([k, v]) => k + ": " + v[0] + (v[1] ? " !important" : "") + ";").join(" "));
        const api = {
            getPropertyValue(name) { const v = values().get(dash(name)); return v ? v[0] : ""; },
            getPropertyPriority(name) { const v = values().get(dash(name)); return v ? v[1] : ""; },
            setProperty(name, value, priority = "") { if (snapshot) throw new TypeError("Computed styles are read-only"); name = dash(name); priority = String(priority).toLowerCase(); if (priority && priority !== "important") return; const map = values(); if (value == null || value === "") map.delete(name); else map.set(name, [str(value), priority]); write(map); },
            removeProperty(name) { const old = this.getPropertyValue(name); this.setProperty(name, ""); return old; },
            item(index) { return [...values().keys()][index] || ""; },
            get length() { return values().size; }, get cssText() { return [...values()].map(([k, v]) => k + ": " + v[0] + (v[1] ? " !important" : "") + ";").join(" "); },
            set cssText(v) { if (snapshot) throw new TypeError("Computed styles are read-only"); node.setAttribute("style", str(v)); },
        };
        return new Proxy(api, {
            get(o, k) { if (typeof k === "symbol" || k in o) return Reflect.get(o, k); if (/^\d+$/.test(k)) return o.item(Number(k)); return o.getPropertyValue(k === "cssFloat" ? "float" : k); },
            set(o, k, v) { if (k in o) return Reflect.set(o, k, v); o.setProperty(k === "cssFloat" ? "float" : k, v); return true; },
            has(o, k) { return k in o || values().has(dash(k)); }
        });
    }

    function splitOutside(text, separator) {
        const parts = []; let start = 0, depth = 0, quote = "";
        for (let i = 0; i < text.length; ++i) { const c = text[i]; if (c === "\\") { ++i; continue; } if (quote) { if (c === quote) quote = ""; } else if (c === '"' || c === "'") quote = c; else if (c === "[" || c === "(") ++depth; else if (c === "]" || c === ")") --depth; else if (!depth && c === separator) { parts.push(text.slice(start, i)); start = i + 1; } if (depth < 0) throw new SyntaxError("Invalid selector"); }
        if (quote || depth) throw new SyntaxError("Invalid selector"); parts.push(text.slice(start)); return parts;
    }
    const unescapeCss = s => s.replace(/\\([0-9a-fA-F]{1,6}\s?|.)/g, (_, c) => /^[0-9a-fA-F]/.test(c) ? String.fromCodePoint(parseInt(c, 16) || 0xfffd) : c);
    function selectorParts(selector) {
        const out = []; let start = 0, depth = 0, quote = "", relation = " ";
        const flush = end => { const s = selector.slice(start, end).trim(); if (s) { out.push({ simple: s, relation }); relation = " "; } };
        for (let i = 0; i < selector.length; ++i) { const c = selector[i]; if (c === "\\") { ++i; continue; } if (quote) { if (c === quote) quote = ""; continue; } if (c === '"' || c === "'") quote = c; else if (c === "[" || c === "(") ++depth; else if (c === "]" || c === ")") --depth; else if (!depth && (/\s/.test(c) || ">+~".includes(c))) { flush(i); if (">+~".includes(c)) relation = c; start = i + 1; } }
        flush(selector.length); if (!out.length || depth || quote) throw new SyntaxError("Invalid selector"); return out;
    }
    function simpleMatches(node, selector, scope) {
        if (!node || node.nodeType !== 1) return false;
        let i = 0;
        const ident = () => { const m = /^(?:[\w\u0080-\uffff-]|\\(?:[0-9a-fA-F]{1,6}\s?|.))+/.exec(selector.slice(i)); if (!m) throw new SyntaxError("Invalid selector identifier"); i += m[0].length; return unescapeCss(m[0]); };
        if (selector[i] === "*") ++i;
        else if (selector[i] && !".#[:".includes(selector[i])) { if (ident().toLowerCase() !== record(node).tag) return false; }
        while (i < selector.length) {
            const kind = selector[i++];
            if (kind === "#") { if (node.id !== ident()) return false; }
            else if (kind === ".") { if (!node.classList.contains(ident())) return false; }
            else if (kind === "[") {
                const start = i; let quote = "";
                while (i < selector.length) { const c = selector[i]; if (c === "\\") ++i; else if (quote) { if (c === quote) quote = ""; } else if (c === '"' || c === "'") quote = c; else if (c === "]") break; ++i; }
                if (i === selector.length) throw new SyntaxError("Invalid attribute selector");
                const raw = selector.slice(start, i++), m = /^\s*([^\s~|^$*=]+)\s*(?:([~|^$*]?=)\s*(?:"((?:\\.|[^"])*)"|'((?:\\.|[^'])*)'|([^\s]+))\s*([iIsS])?)?\s*$/.exec(raw);
                if (!m) throw new SyntaxError("Invalid attribute selector");
                let actual = node.getAttribute(unescapeCss(m[1])), wanted = unescapeCss(m[3] ?? m[4] ?? m[5] ?? "");
                if (actual === null) return false; if (!m[2]) continue;
                if (m[6] && m[6].toLowerCase() === "i") { actual = actual.toLowerCase(); wanted = wanted.toLowerCase(); }
                if (!(m[2] === "=" ? actual === wanted : m[2] === "~=" ? wanted && actual.split(/\s+/).includes(wanted) : m[2] === "|=" ? actual === wanted || actual.startsWith(wanted + "-") : m[2] === "^=" ? wanted && actual.startsWith(wanted) : m[2] === "$=" ? wanted && actual.endsWith(wanted) : wanted && actual.includes(wanted))) return false;
            } else if (kind === ":") {
                const name = ident().toLowerCase(); let argument = null;
                if (selector[i] === "(") { let depth = 1, quote = ""; const start = ++i; while (i < selector.length && depth) { const c = selector[i]; if (c === "\\") ++i; else if (quote) { if (c === quote) quote = ""; } else if (c === '"' || c === "'") quote = c; else if (c === "(") ++depth; else if (c === ")") --depth; if (depth) ++i; } if (depth) throw new SyntaxError("Invalid pseudo selector"); argument = selector.slice(start, i++).trim(); }
                const siblings = node.parentNode ? node.parentNode.children : [node], same = siblings.filter(n => n.tagName === node.tagName), position = siblings.indexOf(node) + 1;
                if (name === "not" || name === "is" || name === "where") { const yes = matchesSelector(node, argument, scope); if (name === "not" ? yes : !yes) return false; }
                else if (name === "scope") { if (node !== scope) return false; }
                else if (name === "root") { if (node !== document.documentElement) return false; }
                else if (name === "first-child") { if (position !== 1) return false; }
                else if (name === "last-child") { if (position !== siblings.length) return false; }
                else if (name === "only-child") { if (siblings.length !== 1) return false; }
                else if (name === "first-of-type") { if (same[0] !== node) return false; }
                else if (name === "last-of-type") { if (same[same.length - 1] !== node) return false; }
                else if (name === "only-of-type") { if (same.length !== 1) return false; }
                else if (["nth-child", "nth-last-child", "nth-of-type", "nth-last-of-type"].includes(name)) {
                    const list = name.includes("of-type") ? same : siblings; let p = list.indexOf(node) + 1; if (name.includes("last")) p = list.length - p + 1;
                    const expr = (argument || "").replace(/\s+/g, "").toLowerCase();
                    let a = 0, b = 0;
                    if (expr === "odd") { a = 2; b = 1; } else if (expr === "even") a = 2;
                    else if (/^[+-]?\d+$/.test(expr)) b = Number(expr);
                    else { const m = /^([+-]?\d*)n([+-]\d+)?$/.exec(expr); if (!m) throw new SyntaxError("Invalid nth selector"); a = m[1] === "" || m[1] === "+" ? 1 : m[1] === "-" ? -1 : Number(m[1]); b = Number(m[2] || 0); }
                    if (a === 0 ? p !== b : (p - b) / a < 0 || (p - b) % a !== 0) return false;
                } else if (name === "empty") { if (record(node).children.some(n => n.nodeType === 1 || n.nodeType === 3 && n.nodeValue.length)) return false; }
                else if (name === "checked") { if (!node.checked && !(record(node).tag === "option" && node.selected)) return false; }
                else if (name === "disabled" || name === "enabled") { if (!["button", "input", "select", "textarea", "option", "optgroup", "fieldset"].includes(record(node).tag) || (name === "disabled" ? !node.disabled : node.disabled)) return false; }
                else if (name === "has") { if (!node.querySelector(argument)) return false; }
                else throw new SyntaxError("Unsupported pseudo selector: " + name);
            } else throw new SyntaxError("Invalid compound selector");
        }
        return true;
    }
    function matchesSelector(node, selector, scope = node) {
        return splitOutside(String(selector), ",").some(group => {
            const parts = selectorParts(group.trim());
            const match = (n, index) => {
                if (!simpleMatches(n, parts[index].simple, scope)) return false;
                if (!index) return true;
                const relation = parts[index].relation;
                if (relation === ">") return match(n.parentElement, index - 1);
                if (relation === "+") return match(n.previousElementSibling, index - 1);
                if (relation === "~") { for (let p = n.previousElementSibling; p; p = p.previousElementSibling) if (match(p, index - 1)) return true; return false; }
                for (let p = n.parentElement; p; p = p.parentElement) if (match(p, index - 1)) return true;
                return false;
            };
            return match(node, parts.length - 1);
        });
    }
    Node.prototype.matches = function (selector) { return matchesSelector(this, selector); };
    Node.prototype.msMatchesSelector = Node.prototype.webkitMatchesSelector = Node.prototype.matches;
    Node.prototype.closest = function (selector) { for (let n = this; n && n.nodeType === 1; n = n.parentElement) if (n.matches(selector)) return n; return null; };
    Node.prototype.querySelector = function (selector) { selectorParts(String(selector)); return descendants(this, n => matchesSelector(n, selector, this))[0] || null; };
    Node.prototype.querySelectorAll = function (selector) { selectorParts(String(selector)); return descendants(this, n => matchesSelector(n, selector, this)); };

    const voidTags = new Set("area base br col embed hr img input link meta param source track wbr".split(" "));
    const rawTags = new Set(["script", "style"]);
    const entities = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: "\u00a0", copy: "\u00a9", reg: "\u00ae", hellip: "\u2026", ndash: "\u2013", mdash: "\u2014", lsquo: "\u2018", rsquo: "\u2019", ldquo: "\u201c", rdquo: "\u201d", bull: "\u2022", times: "\u00d7", divide: "\u00f7", euro: "\u20ac" };
    function decodeEntities(text) { return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z][a-z\d]+);/gi, (all, name) => { if (name[0] !== "#") return own(entities, name) ? entities[name] : all; let code = name[1].toLowerCase() === "x" ? parseInt(name.slice(2), 16) : Number(name.slice(1)); if (!code || code > 0x10ffff || code >= 0xd800 && code <= 0xdfff) code = 0xfffd; return String.fromCodePoint(code); }); }
    const escapeText = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    function serialize(node) {
        const r = record(node);
        if (r.type === 3) return r.parent && rawTags.has(record(r.parent).tag) ? r.text : escapeText(r.text);
        if (r.type === 8) return "<!--" + r.text + "-->";
        if (r.type === 9 || r.type === 11) return r.children.map(serialize).join("");
        const attrs = Object.entries(r.attrs).map(([k, v]) => " " + k + '="' + escapeText(v).replace(/"/g, "&quot;") + '"').join("");
        return "<" + r.tag + attrs + ">" + (voidTags.has(r.tag) ? "" : r.children.map(serialize).join("") + "</" + r.tag + ">");
    }
    function parseFragment(html) {
        const fragment = create(11, "", ""), stack = [fragment]; let index = 0;
        const addText = (text, raw = false) => { if (text) stack[stack.length - 1].appendChild(document.createTextNode(raw ? text : decodeEntities(text))); };
        while (index < html.length) {
            const parent = stack[stack.length - 1], tag = record(parent).tag;
            if (rawTags.has(tag) || tag === "textarea" || tag === "title") {
                const re = new RegExp("</" + tag + "\\s*>", "ig"); re.lastIndex = index; const end = re.exec(html);
                addText(html.slice(index, end ? end.index : html.length), rawTags.has(tag)); index = end ? re.lastIndex : html.length; stack.pop(); continue;
            }
            if (html.startsWith("<!--", index)) { const end = html.indexOf("-->", index + 4); parent.appendChild(document.createComment(html.slice(index + 4, end < 0 ? html.length : end))); index = end < 0 ? html.length : end + 3; continue; }
            if (html[index] !== "<") { const end = html.indexOf("<", index); addText(html.slice(index, end < 0 ? html.length : end)); index = end < 0 ? html.length : end; continue; }
            const close = /^<\/\s*([\w:-]+)[^>]*>/.exec(html.slice(index));
            if (close) { const name = close[1].toLowerCase(); for (let i = stack.length - 1; i > 0; --i) if (record(stack[i]).tag === name) { stack.length = i; break; } index += close[0].length; continue; }
            if (/^<!|^<\?/.test(html.slice(index))) { const end = html.indexOf(">", index + 2); index = end < 0 ? html.length : end + 1; continue; }
            const open = /^<([a-zA-Z][\w:-]*)/.exec(html.slice(index));
            if (!open) { addText("<"); ++index; continue; }
            const name = open[1].toLowerCase(), node = document.createElement(name); index += open[0].length;
            while (index < html.length) {
                const ws = /^\s+/.exec(html.slice(index)); if (ws) index += ws[0].length;
                if (html[index] === ">" || html.startsWith("/>", index)) break;
                const attr = /^([^\s\0"'>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/.exec(html.slice(index));
                if (!attr) { ++index; continue; }
                const key = attr[1].toLowerCase(); if (!node.hasAttribute(key)) node.setAttribute(key, decodeEntities(attr[2] ?? attr[3] ?? attr[4] ?? "")); index += attr[0].length;
            }
            const selfClose = html.startsWith("/>", index); index += selfClose ? 2 : html[index] === ">" ? 1 : 0;
            if (name === "script") record(node).inertScript = true;
            if ((name === "li" && tag === "li") || (["dt", "dd"].includes(name) && ["dt", "dd"].includes(tag)) || (name === "p" && tag === "p") || (name === "option" && tag === "option")) stack.pop();
            stack[stack.length - 1].appendChild(node);
            if (!voidTags.has(name)) stack.push(node);
        }
        return fragment;
    }
    Node.prototype.insertAdjacentHTML = function (position, html) { const f = parseFragment(str(html)); switch (String(position).toLowerCase()) { case "beforebegin": if (this.parentNode) this.parentNode.insertBefore(f, this); break; case "afterbegin": this.insertBefore(f, this.firstChild); break; case "beforeend": this.appendChild(f); break; case "afterend": if (this.parentNode) this.parentNode.insertBefore(f, this.nextSibling); break; default: throw new SyntaxError("Invalid position"); } };

    function resolveUrl(value, base = environment && environment.url) {
        value = str(value).trim();
        if (!base && !/^[a-z][a-z\d+.-]*:/i.test(value)) return value;
        const absolute = /^([a-z][a-z\d+.-]*:)(?:\/\/([^/?#]*))?([^?#]*)(\?[^#]*)?(#.*)?$/i;
        const b = absolute.exec(base || ""), v = absolute.exec(value);
        if (v) return normalize(v);
        if (!b) throw new TypeError("Invalid URL");
        if (value.startsWith("//")) return resolveUrl(b[1] + value, base);
        const hashAt = value.indexOf("#"), hash = hashAt < 0 ? "" : value.slice(hashAt), beforeHash = hashAt < 0 ? value : value.slice(0, hashAt);
        const queryAt = beforeHash.indexOf("?"), query = queryAt < 0 ? null : beforeHash.slice(queryAt), path = queryAt < 0 ? beforeHash : beforeHash.slice(0, queryAt);
        return normalize([null, b[1], b[2], path.startsWith("/") ? path : path ? (b[3] || "/").replace(/[^/]*$/, "") + path : b[3], query === null ? path ? "" : b[4] : query, hash]);
        function normalize(m) {
            const protocol = m[1].toLowerCase(), authority = m[2] === undefined ? undefined : m[2].toLowerCase();
            let path = m[3] || (authority === undefined ? "" : "/");
            if (authority !== undefined) { const trailing = /\/(?:\.|\.\.)?$/.test(path), parts = []; for (const part of path.split("/")) { if (part === "..") { if (parts.length > 1) parts.pop(); } else if (part !== ".") parts.push(part); } path = parts.join("/") || "/"; if (trailing && !path.endsWith("/")) path += "/"; }
            return protocol + (authority === undefined ? "" : "//" + authority) + path + (m[4] || "") + (m[5] || "");
        }
    }
    function urlParts(value) {
        const href = resolveUrl(value), m = /^([a-z][a-z\d+.-]*:)\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/i.exec(href);
        if (!m) return { href, protocol: href.split(":")[0] + ":", host: "", hostname: "", port: "", pathname: "", search: "", hash: "", origin: "null" };
        const host = m[2].replace(/^.*@/, ""), port = /:(\d+)$/.exec(host);
        return { href, protocol: m[1], host, hostname: port ? host.slice(0, -port[0].length) : host, port: port ? port[1] : "", pathname: m[3] || "/", search: m[4] || "", hash: m[5] || "", origin: m[1] + "//" + host };
    }
    const location = {};
    for (const name of ["href", "protocol", "host", "hostname", "port", "pathname", "search", "hash", "origin"]) Object.defineProperty(location, name, { enumerable: true, get() { if (!environment) throw new Error("Browser environment unavailable"); return urlParts(environment.url)[name]; }, set() { throw new Error("Navigation is unsupported"); } });
    Object.assign(location, { toString() { return this.href; }, assign() { throw new Error("Navigation is unsupported"); }, replace() { throw new Error("Navigation is unsupported"); }, reload() { throw new Error("Navigation is unsupported"); } });
    Object.freeze(location);
    Object.defineProperty(globalThis, "location", { get: () => location, set() { throw new Error("Navigation is unsupported"); } });
    const navigator = {};
    Object.defineProperties(navigator, { userAgent: { get() { if (!environment) throw new Error("Browser environment unavailable"); return environment.user_agent; } }, cookieEnabled: { get() { return !!environment; } } });
    Object.freeze(navigator);
    Object.defineProperty(globalThis, "navigator", { value: navigator });
    for (const name of ["window", "self", "top", "parent"]) Object.defineProperty(globalThis, name, { value: globalThis });
    Object.defineProperties(globalThis, {
        innerWidth: { get: () => width }, innerHeight: { get: () => height },
        scrollY: { get: () => scrollY }, pageYOffset: { get: () => scrollY }, scrollX: { get: () => 0 }, pageXOffset: { get: () => 0 }, devicePixelRatio: { value: 1 },
    });
    for (const name of ["name", "type", "rel", "media", "target", "title", "lang", "dir", "placeholder", "content", "httpEquiv", "charset", "method", "action", "enctype"]) Object.defineProperty(Node.prototype, name, { configurable: true, get() { return this.getAttribute(name === "httpEquiv" ? "http-equiv" : name) || ""; }, set(v) { this.setAttribute(name === "httpEquiv" ? "http-equiv" : name, v); } });
    for (const name of ["src", "href"]) Object.defineProperty(Node.prototype, name, { get() { const value = this.getAttribute(name); return value === null ? "" : resolveUrl(value); }, set(v) { this.setAttribute(name, v); } });
    for (const name of ["protocol", "host", "hostname", "port", "pathname", "search", "hash", "origin"]) Object.defineProperty(Node.prototype, name, { get() { return urlParts(this.href)[name]; } });
    for (const name of ["async", "defer", "disabled", "multiple", "required", "readOnly", "hidden"]) Object.defineProperty(Node.prototype, name, { get() { return this.hasAttribute(name.toLowerCase()); }, set(v) { if (v) this.setAttribute(name.toLowerCase(), ""); else this.removeAttribute(name.toLowerCase()); } });
    for (const name of ["checked", "selected"]) {
        Object.defineProperty(Node.prototype, name, { get() { const r = record(this); return own(r, name + "State") ? r[name + "State"] : this.hasAttribute(name); }, set(v) { record(this)[name + "State"] = !!v; if (v) this.setAttribute(name, ""); else this.removeAttribute(name); } });
        Object.defineProperty(Node.prototype, "default" + name[0].toUpperCase() + name.slice(1), { get() { return this.hasAttribute(name); }, set(v) { if (v) this.setAttribute(name, ""); else this.removeAttribute(name); } });
    }
    Object.defineProperties(Node.prototype, {
        value: { get() { const r = record(this); if (own(r, "valueState")) return r.valueState; if (r.tag === "textarea") return this.textContent; if (r.tag === "option") return this.getAttribute("value") ?? this.textContent.trim(); if (r.tag === "select") { const option = this.options.find(n => n.selected) || this.options[0]; return option ? option.value : ""; } return this.getAttribute("value") ?? (["checkbox", "radio"].includes(this.type) ? "on" : ""); }, set(v) { const text = str(v); record(this).valueState = text; this.setAttribute("value", text); } },
        defaultValue: { get() { return record(this).tag === "textarea" ? this.textContent : this.getAttribute("value") || ""; }, set(v) { if (record(this).tag === "textarea") this.textContent = v; else this.setAttribute("value", v); } },
        options: { get() { return this.getElementsByTagName("option"); } },
        selectedIndex: { get() { return this.options.findIndex(n => n.selected); }, set(v) { this.options.forEach((n, i) => { n.selected = i === Number(v); }); } },
        form: { get() { for (let p = this.parentElement; p; p = p.parentElement) if (record(p).tag === "form") return p; return null; } },
        elements: { get() { return descendants(this, n => ["input", "textarea", "select", "button", "fieldset", "object", "output"].includes(record(n).tag)); } },
    });

    function nativeLayout(node, requireStyles = false) {
        record(node);
        if (!connected(node)) return null;
        if (mutations.length || !layouts.has(record(node).id) || requireStyles && !layouts.get(record(node).id).styles.length) {
            const wire = mutations.join("\n");
            mutations = []; mutationBytes = 0;
            const frame = __vios_nativeLayout(record(node).id, wire);
            // A synchronous query returns committed geometry, not an event-loop
            // pump: never run timers or lifecycle callbacks recursively here.
            for (const row of frame.layout) {
                if (!nodes.has(row.node)) throw new Error("Native layout returned an unknown node");
                layouts.set(row.node, row);
            }
            cookie = frame.cookie;
        }
        const value = layouts.get(record(node).id);
        if (!value) throw new Error("Native layout snapshot is unavailable for this node");
        return value;
    }
    function rectangle(x, y, w, h) { return Object.freeze({ x, y, width: w, height: h, top: y, left: x, right: x + w, bottom: y + h, toJSON() { return { x, y, width: w, height: h, top: y, left: x, right: x + w, bottom: y + h }; } }); }
    Node.prototype.getBoundingClientRect = function () { const r = nativeLayout(this); return r ? rectangle(r.x, r.y, r.width, r.height) : rectangle(0, 0, 0, 0); };
    Node.prototype.getClientRects = function () { const r = nativeLayout(this); return collection(r && (r.width || r.height) ? [this.getBoundingClientRect()] : []); };
    for (const [name, dimension] of [["offsetWidth", "width"], ["offsetHeight", "height"]]) Object.defineProperty(Node.prototype, name, { get() { const r = nativeLayout(this); return r ? Math.round(r[dimension]) : 0; } });
    for (const [name, dimension, sides] of [["clientWidth", "width", ["left", "right"]], ["clientHeight", "height", ["top", "bottom"]]]) Object.defineProperty(Node.prototype, name, {
        get() {
            const r = nativeLayout(this, true); if (!r) return 0;
            const styles = new Map(r.styles);
            const border = side => { const value = styles.get("border-" + side + "-width"); return value === undefined ? 0 : parseFloat(value) || 0; };
            return Math.max(0, Math.round(r[dimension] - border(sides[0]) - border(sides[1])));
        }
    });
    Object.defineProperties(Node.prototype, {
        offsetParent: { get() { if (!connected(this)) return null; for (let p = this.parentElement; p; p = p.parentElement) { const style = nativeLayout(p, true); if (p === document.body || style && style.styles.some(([k, v]) => k === "position" && v !== "static")) return p; } return null; } },
        offsetLeft: { get() { const r = nativeLayout(this); if (!r) return 0; const p = this.offsetParent, box = p && nativeLayout(p); return r.x - (box ? box.x : 0); } },
        offsetTop: { get() { const r = nativeLayout(this); if (!r) return 0; const p = this.offsetParent, box = p && nativeLayout(p); return r.y - (box ? box.y : 0); } },
    });
    globalThis.getComputedStyle = function (node, pseudo = null) {
        if (pseudo) throw new Error("Pseudo-element computed styles are unsupported");
        const r = nativeLayout(node, true);
        // Detached nodes have no computed render tree. Only inline declarations
        // can be reported, rather than inventing native pixel values.
        return styleDeclaration(node, r ? r.styles : [...declarations(node.getAttribute("style") || "")].map(([k, v]) => [k, v[0]]));
    };

    function activateTree(node) { activate(node); for (const child of record(node).children) activateTree(child); }
    function cancelResource(node) {
        const r = record(node), resource = r.resource;
        if (!resource || !pending.has(resource.id)) return;
        pending.delete(resource.id); r.resource = null;
        request("A", resource.id);
    }
    function cancelTree(node) {
        const r = record(node);
        if (r.tag === "script" || r.tag === "link") cancelResource(node);
        for (const child of r.children) cancelTree(child);
    }
    function activate(node) {
        if (!environment) return;
        const r = record(node); if (r.type !== 1) return;
        if (r.resource) {
            const raw = r.tag === "link" ? r.attrs.href : r.attrs.src;
            const valid = raw && resolveUrl(raw) === r.resource.url
                && (r.tag !== "link" || (r.attrs.rel || "").toLowerCase().split(/\s+/).includes("stylesheet"));
            if (!valid && pending.has(r.resource.id)) {
                cancelResource(node);
                if (r.tag === "script") r.scriptStarted = false;
            }
        }
        let kind, url;
        if (r.tag === "script") {
            if (!connected(node) || r.scriptStarted || r.inertScript || !["", "text/javascript", "application/javascript", "application/ecmascript", "text/ecmascript"].includes((r.attrs.type || "").trim().toLowerCase())) return;
            url = r.attrs.src;
            if (!url) {
                if (!node.textContent) return;
                r.scriptStarted = true; const previous = currentScript; currentScript = node;
                try { globalEval(node.textContent); } finally { currentScript = previous; }
                return;
            }
            kind = "Script";
        } else if (r.tag === "link") {
            if (!connected(node) || !(r.attrs.rel || "").toLowerCase().split(/\s+/).includes("stylesheet") || !r.attrs.href) return;
            kind = "Stylesheet"; url = r.attrs.href;
        } else if (r.tag === "img") { if (!r.attrs.src) return; kind = "Image"; url = r.attrs.src; }
        else return;
        url = resolveUrl(url);
        if (r.resource && r.resource.url === url) return;
        if (r.resource && pending.has(r.resource.id)) { request("A", r.resource.id); pending.delete(r.resource.id); }
        const id = allocateRequest(); request("L", id, r.id, url, kind);
        pending.set(id, { type: "load", node, kind }); r.resource = { id, url };
        if (kind === "Script") r.scriptStarted = true;
    }
    function lifecycle() {
        if (parsingComplete && readyState === "loading") { readyState = "interactive"; fire(document, "readystatechange"); fire(document, "DOMContentLoaded", { bubbles: true }); }
        if (parsingComplete && !loaded && ![...pending.values()].some(p => p.type === "load")) { loaded = true; readyState = "complete"; fire(document, "readystatechange"); fire(globalThis, "load"); }
    }
    function schedule(type, handler, delay, args, timeout) {
        if (!environment) throw new Error("Native browser environment is not configured");
        if (timers.size >= MAX_TIMERS || nextTimer > 0x7fffffff) throw new RangeError("Browser timer limit exceeded");
        if (typeof handler !== "function") { const source = String(handler); handler = () => globalEval(source); }
        const id = nextTimer++, milliseconds = Number(delay);
        const interval = Number.isFinite(milliseconds) ? Math.max(0, Math.min(2147483647, milliseconds)) : 0;
        timers.set(id, { type, handler, args, timeoutRequested: timeout !== undefined, interval: type === "interval" ? Math.max(1, interval) : interval, due: clock + (type === "raf" ? 16 : type === "idle" ? Math.min(timeout ?? 1, 2147483647) : interval) });
        return id;
    }
    globalThis.setTimeout = (handler, delay = 0, ...args) => schedule("timeout", handler, delay, args);
    globalThis.setInterval = (handler, delay = 0, ...args) => schedule("interval", handler, delay, args);
    globalThis.clearTimeout = globalThis.clearInterval = id => { timers.delete(Number(id)); };
    globalThis.requestAnimationFrame = handler => { if (typeof handler !== "function") throw new TypeError("Callback required"); return schedule("raf", handler, 16, []); };
    globalThis.cancelAnimationFrame = id => { timers.delete(Number(id)); };
    globalThis.requestIdleCallback = (handler, options = {}) => { if (typeof handler !== "function") throw new TypeError("Callback required"); const timeout = options.timeout === undefined ? undefined : Math.max(0, Number(options.timeout) || 0); return schedule("idle", handler, 0, [], timeout); };
    globalThis.cancelIdleCallback = id => { timers.delete(Number(id)); };
    const performance = Object.freeze({ now: () => clock });
    Object.defineProperty(globalThis, "performance", { value: performance });
    const mediaLists = new Set();
    function mediaMatches(query) {
        return splitOutside(String(query).toLowerCase(), ",").some(group => {
            let text = group.trim(), invert = false;
            if (text.startsWith("not ")) { invert = true; text = text.slice(4).trim(); }
            if (text.startsWith("only ")) text = text.slice(5).trim();
            const parts = text.split(/\s+and\s+/), type = parts[0].startsWith("(") ? "all" : parts.shift();
            let result = type === "all" || type === "screen";
            for (const part of parts) {
                const m = /^\(\s*([\w-]+)\s*:\s*([^)]+)\s*\)$/.exec(part.trim());
                if (!m) throw new SyntaxError("Unsupported media query");
                const name = m[1], value = m[2].trim();
                if (name === "orientation") { if (!["landscape", "portrait"].includes(value)) throw new SyntaxError("Invalid orientation"); result = result && (value === "landscape" ? width > height : height >= width); continue; }
                const size = /^([+-]?(?:\d+\.?\d*|\.\d+))(px|em|rem|dppx|dpi)?$/.exec(value);
                if (!size) throw new SyntaxError("Unsupported media query value");
                let numeric = Number(size[1]); if (size[2] === "em" || size[2] === "rem") numeric *= 16; if (size[2] === "dpi") numeric /= 96;
                const feature = name.replace(/^(min|max)-/, "");
                const actual = feature === "width" ? width : feature === "height" ? height : feature === "resolution" ? 1 : null;
                if (actual === null) throw new SyntaxError("Unsupported media feature");
                result = result && (name.startsWith("min-") ? actual >= numeric : name.startsWith("max-") ? actual <= numeric : actual === numeric);
            }
            return invert ? !result : result;
        });
    }
    globalThis.matchMedia = function (query) {
        query = str(query); const initial = mediaMatches(query);
        if (mediaLists.size >= 128) throw new RangeError("Media query limit exceeded");
        const list = Object.create(EventTarget.prototype);
        Object.defineProperties(list, { media: { value: query, enumerable: true }, matches: { get: () => mediaMatches(query), enumerable: true } });
        list.addListener = handler => list.addEventListener("change", handler);
        list.removeListener = handler => list.removeEventListener("change", handler);
        mediaLists.add({ list, previous: initial }); return list;
    };

    function abortError(reason) { if (reason !== undefined) return reason; const error = new Error("The operation was aborted"); error.name = "AbortError"; return error; }
    class AbortSignal extends EventTarget {
        constructor(secret) { super(); if (secret !== abortSecret) throw new TypeError("Illegal constructor"); this.aborted = false; this.reason = undefined; }
        throwIfAborted() { if (this.aborted) throw this.reason; }
    }
    const abortSecret = {};
    class AbortController {
        constructor() { Object.defineProperty(this, "signal", { value: new AbortSignal(abortSecret) }); }
        abort(reason) { if (this.signal.aborted) return; this.signal.aborted = true; this.signal.reason = abortError(reason); fire(this.signal, "abort"); }
    }
    function headerName(name) { name = str(name).toLowerCase(); if (!/^[!#$%&'*+.^_`|~0-9a-z-]+$/.test(name)) throw new TypeError("Invalid header name"); return name; }
    function headerValue(value) { value = str(value).trim(); if (/[\0\r\n]/.test(value)) throw new TypeError("Invalid header value"); return value; }
    class Headers {
        constructor(init = []) { this._headers = new Map(); if (init instanceof Headers || Array.isArray(init) || init && typeof init[Symbol.iterator] === "function") { for (const pair of init) { if (pair.length !== 2) throw new TypeError("Header pair required"); this.append(pair[0], pair[1]); } } else for (const key of Object.keys(init)) this.append(key, init[key]); }
        append(name, value) { name = headerName(name); value = headerValue(value); this._headers.set(name, this._headers.has(name) ? this._headers.get(name) + ", " + value : value); }
        set(name, value) { this._headers.set(headerName(name), headerValue(value)); }
        get(name) { return this._headers.get(headerName(name)) ?? null; }
        has(name) { return this._headers.has(headerName(name)); }
        delete(name) { this._headers.delete(headerName(name)); }
        *entries() { yield* [...this._headers].sort((a, b) => a[0].localeCompare(b[0])); }
        *keys() { for (const [key] of this.entries()) yield key; }
        *values() { for (const [, value] of this.entries()) yield value; }
        forEach(fn, receiver) { for (const [key, value] of this.entries()) fn.call(receiver, value, key, this); }
        [Symbol.iterator]() { return this.entries(); }
    }
    class Response {
        constructor(body = "", init = {}) { this._body = str(body); this.bodyUsed = false; this.status = init.status === undefined ? 200 : Number(init.status); this.headers = new Headers(init.headers); this.url = init.url || ""; this.redirected = !!init.redirected; this.type = init.type || "basic"; this.statusText = init.statusText || ""; }
        get ok() { return this.status >= 200 && this.status < 300; }
        text() { if (this.bodyUsed) return Promise.reject(new TypeError("Response body already consumed")); this.bodyUsed = true; return Promise.resolve(this._body); }
        json() { return this.text().then(JSON.parse); }
        clone() { if (this.bodyUsed) throw new TypeError("Response body already consumed"); return new Response(this._body, { status: this.status, headers: this.headers, url: this.url, redirected: this.redirected, type: this.type, statusText: this.statusText }); }
    }
    globalThis.fetch = function (input, options = {}) {
        return new Promise((resolve, reject) => {
            const url = resolveUrl(typeof input === "string" ? input : String(input)), method = String(options.method || "GET").toUpperCase();
            if (!/^[!#$%&'*+.^_`|~0-9A-Z-]+$/.test(method) || ["CONNECT", "TRACE", "TRACK"].includes(method)) throw new TypeError("Invalid fetch method");
            const headers = new Headers(options.headers), body = options.body == null ? "" : str(options.body), credentials = options.credentials || "same-origin";
            if (!["omit", "same-origin", "include"].includes(credentials)) throw new TypeError("Invalid credentials mode");
            if ((method === "GET" || method === "HEAD") && body) throw new TypeError("GET/HEAD cannot have a body");
            if (options.mode !== undefined && !["cors", "same-origin"].includes(options.mode)) throw new TypeError("Unsupported fetch mode");
            if (options.mode === "same-origin" && urlParts(url).origin !== location.origin) throw new TypeError("Cross-origin request disallowed");
            const signal = options.signal;
            if (signal !== undefined && signal !== null && !(signal instanceof AbortSignal)) throw new TypeError("Invalid AbortSignal");
            if (signal && signal.aborted) { reject(signal.reason); return; }
            const id = allocateRequest(), pairs = [...headers];
            request("F", id, url, method, body, credentials, ...pairs.flat());
            const abort = () => { if (!pending.has(id)) return; request("A", id); pending.delete(id); signal.removeEventListener("abort", abort); reject(signal.reason); };
            pending.set(id, { type: "fetch", resolve, reject, signal, abort, url });
            if (signal) signal.addEventListener("abort", abort, { once: true });
        });
    };
    class XMLHttpRequest extends EventTarget {
        constructor() { super(); this.readyState = 0; this.status = 0; this.statusText = ""; this.responseURL = ""; this.responseText = ""; this.response = ""; this.responseType = ""; this.timeout = 0; this.withCredentials = false; this._sent = false; this._headers = new Headers(); this._responseHeaders = new Headers(); this._generation = 0; }
        open(method, url, async = true, user, password) { if (!async) throw new Error("Synchronous XMLHttpRequest is unsupported"); if (user !== undefined || password !== undefined) throw new Error("XHR URL credentials are unsupported"); if (this._sent) this.abort(); this._method = String(method).toUpperCase(); this._url = resolveUrl(url); this._headers = new Headers(); this._responseHeaders = new Headers(); this.status = 0; this.statusText = ""; this.responseText = this.response = ""; this.readyState = 1; fire(this, "readystatechange"); }
        setRequestHeader(name, value) { if (this.readyState !== 1 || this._sent) throw new Error("InvalidStateError"); this._headers.append(name, value); }
        getResponseHeader(name) { return this.readyState < 2 ? null : this._responseHeaders.get(name); }
        getAllResponseHeaders() { return this.readyState < 2 ? "" : [...this._responseHeaders].map(([k, v]) => k + ": " + v + "\r\n").join(""); }
        overrideMimeType() { throw new Error("XHR MIME overrides are unsupported"); }
        send(body = null) {
            if (this.readyState !== 1 || this._sent) throw new Error("InvalidStateError");
            if (!["", "text", "json"].includes(this.responseType)) throw new Error("Unsupported XHR responseType");
            this._sent = true; const generation = ++this._generation, controller = this._controller = new AbortController(); let timedOut = false;
            const timer = Number(this.timeout) > 0 ? setTimeout(() => { timedOut = true; controller.abort(); }, this.timeout) : null;
            fire(this, "loadstart");
            fetch(this._url, { method: this._method, headers: this._headers, body, credentials: this.withCredentials ? "include" : "same-origin", signal: controller.signal }).then(async response => {
                if (generation !== this._generation) return;
                this.status = response.status; this.statusText = response.statusText; this.responseURL = response.url; this._responseHeaders = response.headers; this.readyState = 2; fire(this, "readystatechange");
                this.readyState = 3; fire(this, "readystatechange"); const text = await response.text(); if (generation !== this._generation) return;
                this.responseText = text; if (this.responseType === "json") { try { this.response = JSON.parse(text); } catch (_) { this.response = null; } } else this.response = text;
                this._sent = false; clearTimeout(timer); this.readyState = 4; fire(this, "readystatechange"); fire(this, "load"); fire(this, "loadend");
            }).catch(error => {
                if (generation !== this._generation) return;
                this._sent = false; clearTimeout(timer); this.status = 0; this.response = this.responseText = ""; this.readyState = 4; fire(this, "readystatechange"); fire(this, timedOut ? "timeout" : controller.signal.aborted ? "abort" : "error"); fire(this, "loadend");
            });
        }
        abort() { if (!this._sent) { this.readyState = 0; return; } this._controller.abort(); }
    }
    Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
    Object.assign(XMLHttpRequest.prototype, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
    Object.assign(globalThis, { Node, Element: Node, HTMLElement: Node, Event, CustomEvent, EventTarget, AbortController, AbortSignal, Headers, Response, XMLHttpRequest });
    globalThis.Image = function (w, h) { const node = document.createElement("img"); if (w !== undefined) node.setAttribute("width", w); if (h !== undefined) node.setAttribute("height", h); return node; };
    const NodeFilter = Object.freeze({
        FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xffffffff,
        SHOW_ELEMENT: 1, SHOW_ATTRIBUTE: 2, SHOW_TEXT: 4, SHOW_CDATA_SECTION: 8,
        SHOW_ENTITY_REFERENCE: 16, SHOW_ENTITY: 32, SHOW_PROCESSING_INSTRUCTION: 64,
        SHOW_COMMENT: 128, SHOW_DOCUMENT: 256, SHOW_DOCUMENT_TYPE: 512, SHOW_DOCUMENT_FRAGMENT: 1024, SHOW_NOTATION: 2048,
    });
    globalThis.NodeFilter = NodeFilter;
    function treeWalker(root, whatToShow = NodeFilter.SHOW_ALL, filter = null) {
        record(root); let current = root;
        const decision = node => {
            if (!(Number(whatToShow) & (1 << (node.nodeType - 1)))) return NodeFilter.FILTER_SKIP;
            if (!filter) return NodeFilter.FILTER_ACCEPT;
            return typeof filter === "function" ? filter(node) : filter.acceptNode(node);
        };
        function visibleChildren(parent) {
            const out = [];
            const visit = node => { const result = decision(node); if (result === NodeFilter.FILTER_ACCEPT) out.push(node); else if (result !== NodeFilter.FILTER_REJECT) for (const child of node.childNodes) visit(child); };
            for (const node of parent.childNodes) visit(node); return out;
        }
        function visibleOrder() {
            const out = [];
            const visit = node => { const result = decision(node); if (result === NodeFilter.FILTER_ACCEPT) out.push(node); if (result !== NodeFilter.FILTER_REJECT) for (const child of node.childNodes) visit(child); };
            for (const node of root.childNodes) visit(node); return out;
        }
        const move = node => { if (node) current = node; return node || null; };
        return {
            root, whatToShow: Number(whatToShow), filter,
            get currentNode() { return current; }, set currentNode(node) { record(node); current = node; },
            parentNode() { for (let p = current.parentNode; p; p = p.parentNode) { if (decision(p) === NodeFilter.FILTER_ACCEPT) return move(p); if (p === root) break; } return null; },
            firstChild() { return move(visibleChildren(current)[0]); },
            lastChild() { const list = visibleChildren(current); return move(list[list.length - 1]); },
            nextSibling() { if (current === root) return null; for (let p = current.parentNode; p; p = p.parentNode) { const list = visibleChildren(p), i = list.indexOf(current); if (i >= 0 && list[i + 1]) return move(list[i + 1]); if (p === root || decision(p) === NodeFilter.FILTER_ACCEPT) return null; } return null; },
            previousSibling() { if (current === root) return null; for (let p = current.parentNode; p; p = p.parentNode) { const list = visibleChildren(p), i = list.indexOf(current); if (i > 0) return move(list[i - 1]); if (p === root || decision(p) === NodeFilter.FILTER_ACCEPT) return null; } return null; },
            nextNode() { const list = visibleOrder(); if (current === root) return move(list[0]); const i = list.indexOf(current); if (i >= 0) return move(list[i + 1]); for (const node of list) if (current.compareDocumentPosition(node) & 4) return move(node); return null; },
            previousNode() { if (current === root) return null; const list = visibleOrder(), i = list.indexOf(current); return i > 0 ? move(list[i - 1]) : null; },
        };
    }
    let resolveFonts;
    const fontReady = new Promise(resolve => { resolveFonts = resolve; });
    const fonts = Object.freeze({
        status: "loaded", size: 0, ready: fontReady,
        *values() {}, *keys() {}, *entries() {}, [Symbol.iterator]() { return this.values(); },
        forEach() {}, has() { return false; },
    });

    function configureDocument(doc) {
        Object.defineProperties(doc, {
            title: { get() { return title; }, set(v) { const text = str(v); emit("T", text); title = text; }, configurable: true },
            documentElement: { get() { return record(doc).children.find(n => record(n).type === 1) || null; }, configurable: true },
            body: { get() { return doc.getElementsByTagName("body")[0] || null; }, configurable: true },
            head: { get() { return doc.getElementsByTagName("head")[0] || null; }, configurable: true },
            defaultView: { value: globalThis, configurable: true },
            URL: { get() { return location.href; }, configurable: true }, documentURI: { get() { return location.href; }, configurable: true }, baseURI: { get() { return location.href; }, configurable: true },
            location: { get: () => location, set() { throw new Error("Navigation is unsupported"); }, configurable: true },
            readyState: { get: () => readyState, configurable: true }, currentScript: { get: () => currentScript, configurable: true },
            cookie: { get: () => cookie, set(value) { value = str(value); if (/[\r\n\0]/.test(value)) throw new TypeError("Invalid cookie"); request("K", value); }, configurable: true },
            fonts: { value: fonts, configurable: true },
        });
        doc.getElementById = id => descendants(doc, n => n.getAttribute("id") === String(id))[0] || null;
        doc.getElementsByName = name => descendants(doc, n => n.getAttribute("name") === String(name));
        doc.createTreeWalker = treeWalker;
        doc.createElement = tag => { tag = String(tag).toLowerCase(); if (!/^[a-z][a-z0-9:_-]*$/.test(tag)) throw new Error("InvalidCharacterError"); return create(1, tag, ""); };
        doc.createTextNode = text => create(3, "", str(text));
        doc.createComment = text => create(8, "", str(text));
        doc.createDocumentFragment = () => create(11, "", "");
        doc.createEvent = type => { if (["Event", "Events", "HTMLEvents", "MouseEvents"].includes(String(type))) return new Event(""); if (String(type) === "CustomEvent") return new CustomEvent(""); throw new Error("Unsupported event interface"); };
        doc.importNode = (node, deep) => node.cloneNode(deep);
    }
    hook("__vios_sync", function (snapshot, newTitle) {
        const old = nodes; nodes = new Map();
        for (const [id, type, tag, text, attrs] of snapshot) {
            let node = old.get(id);
            if (node && records.get(node).type === type) { const r = records.get(node); r.tag = tag; r.text = text; r.attrs = Object.assign(Object.create(null), Object.fromEntries(attrs)); r.parent = null; r.children = []; nodes.set(id, node); }
            else node = makeNode(id, type, tag, text, attrs);
            if (!old.has(id) && type === 1 && tag === "script") record(node).scriptStarted = true;
            globalThis["node_" + id] = node;
        }
        for (const [id] of old) if (!nodes.has(id)) delete globalThis["node_" + id];
        for (const row of snapshot) for (let child = row[6]; child !== null; child = snapshot[child][7]) { record(nodes.get(row[0])).children.push(nodes.get(child)); record(nodes.get(child)).parent = nodes.get(row[0]); }
        nextId = snapshot.length; title = newTitle;
        if (globalThis.document && globalThis.document !== nodes.get(0)) throw new Error("Document identity changed without context reset");
        globalThis.document = nodes.get(0); configureDocument(globalThis.document);
    });
    hook("__vios_evalScript", function (source, id) {
        const node = id === null ? null : nodes.get(id); if (id !== null && (!node || record(node).tag !== "script")) throw new TypeError("Invalid script node");
        const previous = currentScript; currentScript = node;
        try { return globalEval(source); } finally { currentScript = previous; }
    });
    hook("__vios_configure", function (value) {
        if (!globalThis.document || environment) throw new Error("Browser configuration requires a new synchronized document");
        environment = Object.freeze(value); clock = value.now_ms; width = value.width; height = value.height; cookie = value.cookie;
    });
    hook("__vios_pump", function (frame) {
        if (!environment) throw new Error("Browser environment unavailable");
        if (frame.now_ms < clock) throw new RangeError("Browser clock moved backwards");
        const oldWidth = width, oldHeight = height, oldScroll = scrollY;
        clock = frame.now_ms; width = frame.width; height = frame.height; scrollY = frame.scroll_y; cookie = frame.cookie;
        for (const row of frame.layout) if (!nodes.has(row.node)) throw new Error("Layout snapshot refers to an unknown node");
        if (frame.layout.length) layouts = new Map(frame.layout.map(row => [row.node, row]));
        if (resolveFonts) { resolveFonts(fonts); resolveFonts = null; }
        for (const entry of mediaLists) {
            const now = entry.list.matches;
            if (now !== entry.previous) { entry.previous = now; const event = new Event("change"); Object.assign(event, { matches: now, media: entry.list.media }); entry.list.dispatchEvent(event); }
        }
        if (oldWidth !== width || oldHeight !== height) fire(globalThis, "resize");
        if (oldScroll !== scrollY) fire(globalThis, "scroll");
        parsingComplete = parsingComplete || frame.parsing_complete; lifecycle();
        const due = [...timers].filter(([, timer]) => timer.due <= clock).sort((a, b) => a[1].due - b[1].due || a[0] - b[0]).slice(0, MAX_CALLBACKS);
        let firstError;
        for (const [id, timer] of due) {
            if (timers.get(id) !== timer) continue;
            if (timer.type === "interval") timer.due = clock + timer.interval; else timers.delete(id);
            try { if (timer.type === "raf") timer.handler.call(globalThis, clock); else if (timer.type === "idle") timer.handler.call(globalThis, { didTimeout: timer.timeoutRequested && timer.due <= clock, timeRemaining: () => 0 }); else timer.handler.apply(globalThis, timer.args); }
            catch (error) { if (firstError === undefined) firstError = error; }
        }
        lifecycle(); if (firstError !== undefined) throw firstError;
    });
    hook("__vios_complete", function (reply) {
        const task = pending.get(reply.id); if (!task) throw new Error("Unknown or stale browser request completion");
        pending.delete(reply.id);
        if (task.type === "fetch") {
            if (task.signal) task.signal.removeEventListener("abort", task.abort);
            if (reply.error !== null) task.reject(new TypeError(reply.error));
            else task.resolve(new Response(reply.body, { status: reply.status, headers: reply.headers, url: reply.url, redirected: reply.url !== task.url }));
        } else {
            const r = record(task.node);
            if (!r.resource || r.resource.id !== reply.id || (task.kind !== "Image" && !connected(task.node))) {
                lifecycle(); return;
            }
            let failure = reply.error !== null || reply.status < 200 || reply.status >= 300, exception;
            if (!failure && task.kind === "Script") {
                const previous = currentScript; currentScript = task.node;
                try { globalEval(reply.body); } catch (error) { failure = true; exception = error; } finally { currentScript = previous; }
            }
            try { fire(task.node, failure ? "error" : "load"); } finally { lifecycle(); }
            if (exception !== undefined) throw exception;
        }
    });
    hook("__vios_takeMutations", function () { const out = mutations.join("\n"); mutations = []; mutationBytes = 0; return out; });
    hook("__vios_takeRequests", function () { const out = requests.join("\n"); requests = []; requestBytes = 0; return out; });
    hook("__vios_nextWakeup", function () { let value = null; for (const timer of timers.values()) if (value === null || timer.due < value) value = timer.due; return value === null ? "" : String(Math.ceil(value)); });
    hook("__vios_takeLogs", function () { const out = logs.join("\n"); logs = []; logBytes = 0; return out; });
    hook("__vios_dispatch", function (id, kind, x, y, key) { const target = nodes.get(id); if (!target) throw new Error("Event target does not exist"); const event = new Event(kind, { bubbles: true, cancelable: true }); Object.assign(event, { clientX: x, clientY: y, key }); return target.dispatchEvent(event); });
    globalThis.console = { log(...args) { log("[script] ", args); }, warn(...args) { log("[warn] ", args); }, error(...args) { log("[error] ", args); }, info(...args) { log("[info] ", args); } };
})();
