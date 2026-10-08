/* SPDX-License-Identifier: MIT
 * Document-backed DOM subset. No network, navigation, timers or fake Web APIs.
 * Strings in the mutation wire are percent-encoded so tabs/newlines/NUL survive.
 */
(function () {
    "use strict";
    let nodes = new Map(), records = new WeakMap(), nextId = 0;
    let mutations = [], logs = [], title = "";
    // Arena strings are Unicode scalar UTF-8, so lone JS surrogates become U+FFFD.
    const str = value => String(value).toWellFormed();
    const enc = value => encodeURIComponent(str(value));
    function emit(kind, ...fields) { mutations.push([kind, ...fields.map(enc)].join("\t")); }
    function record(node) {
        const r = records.get(node);
        if (!r || nodes.get(r.id) !== node) throw new TypeError("Node belongs to a different document");
        return r;
    }
    function detach(node) {
        const r = record(node);
        if (r.parent) {
            const children = record(r.parent).children;
            children.splice(children.indexOf(node), 1);
            r.parent = null;
        }
    }
    function append(parent, child) {
        const p = record(parent), c = record(child);
        if ((p.type !== 1 && p.type !== 9) || c.type === 9) throw new Error("HierarchyRequestError");
        for (let cursor = parent; cursor; cursor = record(cursor).parent) {
            if (cursor === child) throw new Error("HierarchyRequestError");
        }
        detach(child);
        p.children.push(child);
        c.parent = parent;
    }
    function textOf(node) {
        const r = record(node);
        if (r.type === 9) return null;
        if (r.type === 3 || r.type === 8) return r.text;
        let text = "", stack = r.children.slice().reverse();
        while (stack.length) {
            const current = record(stack.pop());
            if (current.type === 3) text += current.text;
            for (let i = current.children.length - 1; i >= 0; --i) stack.push(current.children[i]);
        }
        return text;
    }
    function makeNode(id, type, tag, text, attrs) {
        const node = Object.create(Node.prototype);
        records.set(node, { id, type, tag, text, attrs: Object.assign(Object.create(null), Object.fromEntries(attrs)), parent: null, children: [], listeners: [] });
        nodes.set(id, node);
        globalThis["node_" + id] = node;
        return node;
    }
    function create(type, tag, text, notify = true) {
        const id = nextId++;
        const node = makeNode(id, type, tag, text, []);
        if (notify) emit("C", id, type, tag, text);
        return node;
    }
    function Node() { throw new TypeError("Use document.createElement/createTextNode/createComment"); }
    Object.defineProperties(Node.prototype, {
        nodeType: { get() { return record(this).type; } },
        nodeName: { get() { const r = record(this); return r.type === 1 ? r.tag.toUpperCase() : r.type === 3 ? "#text" : r.type === 8 ? "#comment" : "#document"; } },
        parentNode: { get() { return record(this).parent; } },
        firstChild: { get() { return record(this).children[0] || null; } },
        lastChild: { get() { const c = record(this).children; return c[c.length - 1] || null; } },
        childNodes: { get() { return record(this).children.slice(); } },
        children: { get() { return record(this).children.filter(n => record(n).type === 1); } },
        nextSibling: { get() { const p = this.parentNode; return p ? record(p).children[record(p).children.indexOf(this) + 1] || null : null; } },
        previousSibling: { get() { const p = this.parentNode; return p ? record(p).children[record(p).children.indexOf(this) - 1] || null : null; } },
        textContent: {
            get() { return textOf(this); },
            set(value) {
                const r = record(this), text = value == null ? "" : str(value);
                if (r.type === 9) return;
                if (r.type === 3 || r.type === 8) r.text = text;
                else {
                    for (const child of r.children) record(child).parent = null;
                    r.children = [];
                    // Arena SetText allocates exactly this ID; no duplicate CreateNode.
                    if (text) append(this, create(3, "", text, false));
                }
                emit("X", r.id, text);
            }
        },
        id: { get() { return this.getAttribute("id") || ""; }, set(v) { this.setAttribute("id", v); } },
        className: { get() { return this.getAttribute("class") || ""; }, set(v) { this.setAttribute("class", v); } }
    });
    Node.prototype.appendChild = function (child) { append(this, child); emit("P", record(this).id, record(child).id); return child; };
    Node.prototype.removeChild = function (child) {
        if (record(child).parent !== this) throw new Error("NotFoundError");
        detach(child); emit("D", record(this).id, record(child).id); return child;
    };
    Node.prototype.remove = function () { const p = this.parentNode; if (p) p.removeChild(this); };
    Node.prototype.setAttribute = function (key, value) {
        const r = record(this);
        if (r.type !== 1) throw new TypeError("Attributes require an element");
        key = str(key).toLowerCase(); value = str(value);
        if (!key || /[\s\0"'>/=]/.test(key)) throw new Error("InvalidCharacterError");
        r.attrs[key] = value; emit("A", r.id, key, value);
    };
    Node.prototype.getAttribute = function (key) {
        const r = record(this); key = String(key).toLowerCase();
        return Object.prototype.hasOwnProperty.call(r.attrs, key) ? r.attrs[key] : null;
    };
    Node.prototype.hasAttribute = function (key) { return this.getAttribute(key) !== null; };
    Node.prototype.removeAttribute = function (key) {
        const r = record(this);
        if (r.type !== 1) throw new TypeError("Attributes require an element");
        key = String(key).toLowerCase(); delete r.attrs[key]; emit("R", r.id, key);
    };
    Node.prototype.addEventListener = function (kind, handler) {
        if (typeof handler !== "function") throw new TypeError("Listener must be a function");
        const list = record(this).listeners;
        kind = String(kind);
        if (!list.some(l => l.kind === kind && l.handler === handler)) list.push({ kind, handler });
    };
    Node.prototype.removeEventListener = function (kind, handler) {
        const r = record(this); r.listeners = r.listeners.filter(l => l.kind !== String(kind) || l.handler !== handler);
    };
    function descendants(root, predicate) {
        const found = [], stack = record(root).children.slice().reverse();
        while (stack.length) {
            const node = stack.pop(), r = record(node);
            if (r.type === 1 && predicate(node)) found.push(node);
            for (let i = r.children.length - 1; i >= 0; --i) stack.push(r.children[i]);
        }
        return found;
    }
    Node.prototype.getElementsByTagName = function (tag) { tag = String(tag).toLowerCase(); return descendants(this, n => tag === "*" || record(n).tag === tag); };
    function configureDocument(doc) {
        Object.defineProperties(doc, {
            title: { get() { return title; }, set(v) { title = str(v); emit("T", title); }, configurable: true },
            documentElement: { get() { return record(doc).children.find(n => record(n).type === 1) || null; }, configurable: true },
            body: { get() { return doc.getElementsByTagName("body")[0] || null; }, configurable: true },
            head: { get() { return doc.getElementsByTagName("head")[0] || null; }, configurable: true }
        });
        doc.getElementById = function (id) { id = String(id); return descendants(doc, n => n.getAttribute("id") === id)[0] || null; };
        doc.createElement = function (tag) {
            tag = String(tag).toLowerCase();
            if (!/^[a-z][a-z0-9:_-]*$/.test(tag)) throw new Error("InvalidCharacterError");
            return create(1, tag, "");
        };
        doc.createTextNode = text => create(3, "", str(text));
        doc.createComment = text => create(8, "", str(text));
    }
    globalThis.__vios_sync = function (snapshot, newTitle) {
        // Reuse wrappers for existing IDs, preserving listeners and JS references.
        const old = nodes; nodes = new Map();
        for (const [id, type, tag, text, attrs] of snapshot) {
            let node = old.get(id);
            if (node && records.get(node).type === type) {
                const r = records.get(node);
                r.tag = tag; r.text = text; r.attrs = Object.assign(Object.create(null), Object.fromEntries(attrs));
                r.parent = null; r.children = [];
                nodes.set(id, node);
            } else node = makeNode(id, type, tag, text, attrs);
            globalThis["node_" + id] = node;
        }
        for (const [id, node] of old) if (!nodes.has(id)) delete globalThis["node_" + id];
        for (const row of snapshot) {
            for (let child = row[6]; child !== null; child = snapshot[child][7]) {
                // Rust validated topology; direct linking avoids quadratic ancestor walks.
                record(nodes.get(row[0])).children.push(nodes.get(child));
                record(nodes.get(child)).parent = nodes.get(row[0]);
            }
        }
        nextId = snapshot.length;
        title = newTitle;
        globalThis.document = nodes.get(0);
        configureDocument(globalThis.document);
    };
    globalThis.__vios_takeMutations = function () { const out = mutations.join("\n"); mutations = []; return out; };
    globalThis.__vios_takeLogs = function () { const out = logs.join("\n"); logs = []; return out; };
    globalThis.__vios_dispatch = function (id, kind, x, y, key) {
        const target = nodes.get(id);
        if (!target) throw new Error("Event target does not exist");
        let stopped = false, prevented = false;
        const event = { type: kind, target, currentTarget: null, clientX: x, clientY: y, key,
            stopPropagation() { stopped = true; }, preventDefault() { prevented = true; },
            get defaultPrevented() { return prevented; } };
        for (let cursor = target; cursor; cursor = record(cursor).parent) {
            event.currentTarget = cursor;
            for (const l of record(cursor).listeners.slice()) if (l.kind === kind) {
                try { l.handler.call(cursor, event); } catch (e) { logs.push("[script] listener error: " + String(e)); }
            }
            if (stopped) break;
        }
        event.currentTarget = null;
    };
    globalThis.console = { log(...args) { logs.push("[script] " + args.map(String).join(" ")); } };
})();
