/* Ocel DOM dialect for the QuickJS engine.
 *
 * Evaluated once per context. It defines exactly the surface the viewer's
 * documents use — the same surface `ocel-js`'s statement matcher implemented,
 * but as real JavaScript, so a document script can use control flow,
 * functions, closures, arrays and the standard library.
 *
 * Every side effect is pushed onto a queue; Rust drains both queues after each
 * evaluation ("<kind>\t<fields…>", records separated by "\n") and turns them
 * into `DomMutation`s. Nothing here talks to the C API: that is what keeps the
 * FFI surface in `ffi.rs` to a dozen entry points.
 */
var __vios_mutations = [];
var __vios_logs = [];
var __vios_listeners = [];
var __vios_title = "";

function __vios_mut(s) { __vios_mutations.push(s); }
function __vios_takeMutations() {
    var out = __vios_mutations.join("\n");
    __vios_mutations = [];
    return out;
}
function __vios_takeLogs() {
    var out = __vios_logs.join("\n");
    __vios_logs = [];
    return out;
}

function __vios_str(v) {
    if (v === undefined) return "undefined";
    if (v === null) return "null";
    return String(v);
}

var console = {
    log: function () {
        var parts = [];
        for (var i = 0; i < arguments.length; i++) parts.push(__vios_str(arguments[i]));
        var line = "[script] " + parts.join(" ");
        __vios_logs.push(line);
    }
};

var document = {
    get title() { return __vios_title; },
    set title(v) {
        __vios_title = __vios_str(v);
        __vios_mut("T\t" + __vios_title);
    },
    /* Documents in the wild (and the fixture) query by id; Ocel routes
       mutations by NodeId, and the viewer's own hit testing decides which node
       a click belongs to, so this returns the matching node_<id> when the id is
       numeric and `null` otherwise. */
    getElementById: function (id) {
        var n = String(id);
        var node = globalThis["node_" + n];
        return node === undefined ? null : node;
    },
    createElement: function (tag) { return __vios_makeNode(-1, tag); },
    addEventListener: function (kind, handler) {
        __vios_listeners.push({ node: -1, kind: String(kind), handler: handler });
    }
};

/* DOM nodes are addressed by arena id (`node_<id>`), which is how Ocel's
   documents and its mutation protocol both name them. */
function __vios_makeNode(id, tag) {
    var node = {
        __id: id,
        __tag: tag || "div",
        __attrs: {},
        get nodeName() { return (this.__tag || "div").toUpperCase(); },
        get textContent() { return this.__text || ""; },
        set textContent(v) {
            this.__text = __vios_str(v);
            __vios_mut("X\t" + this.__id + "\t" + this.__text);
        },
        get innerText() { return this.textContent; },
        set innerText(v) { this.textContent = v; },
        setAttribute: function (key, val) {
            this.__attrs[String(key)] = __vios_str(val);
            __vios_mut("A\t" + this.__id + "\t" + String(key) + "\t" + __vios_str(val));
        },
        getAttribute: function (key) {
            var v = this.__attrs[String(key)];
            return v === undefined ? null : v;
        },
        removeAttribute: function (key) {
            delete this.__attrs[String(key)];
            __vios_mut("R\t" + this.__id + "\t" + String(key));
        },
        appendChild: function (child) {
            __vios_mut("P\t" + this.__id + "\t" + child.__id);
            return child;
        },
        removeChild: function (child) {
            __vios_mut("D\t" + this.__id + "\t" + child.__id);
            return child;
        },
        addEventListener: function (kind, handler) {
            __vios_listeners.push({ node: this.__id, kind: String(kind), handler: handler });
        },
        style: {}
    };
    return node;
}

/* node_0 … node_255, matching NODE_WINDOW in src/engine.rs. */
for (var __vios_i = 0; __vios_i < NODE_WINDOW; __vios_i++) {
    globalThis["node_" + __vios_i] = __vios_makeNode(__vios_i, "div");
}

function __vios_dispatch(node_id, kind) {
    var fired = 0;
    for (var i = 0; i < __vios_listeners.length; i++) {
        var l = __vios_listeners[i];
        if (String(l.kind) !== String(kind)) continue;
        if (l.node !== -1 && l.node !== node_id) continue;
        try {
            l.handler({ type: kind, target: globalThis["node_" + node_id] });
            fired++;
        } catch (e) {
            __vios_logs.push("[script] listener error: " + __vios_str(e));
        }
    }
    return fired;
}
