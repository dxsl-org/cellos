/* SPDX-License-Identifier: MIT
 * Run with tests/host_dom.c against the vendored QuickJS engine, not a JS mock.
 */
(function () {
    function assert(value, message) { if (!value) throw new Error(message); }
    function snapshot() {
        return [
            [0, 9, "", "", [], null, 1, null],
            [1, 1, "html", "", [], 0, 2, null],
            [2, 1, "body", "", [], 1, 4, null],
            [3, 1, "p", "", [["id", "first"]], 2, 5, null],
            [4, 1, "div", "", [["id", "second"]], 2, null, 3],
            [5, 3, "", "hello", [], 3, null, 6],
            [6, 8, "", "not text", [], 3, null, null]
        ];
    }
    __vios_sync(snapshot(), "Initial");
    const first = document.getElementById("first"), second = document.getElementById("second");
    assert(document.getElementById("3") === null, "IDs must be attributes, not numeric NodeIds");
    assert(typeof node_255 === "undefined", "Absent nodes must not be fabricated");
    assert(document.body.firstChild === second && second.nextSibling === first, "Snapshot sibling order, not allocation order");
    assert(first.textContent === "hello", "Element text excludes comments");
    assert(node_6.textContent === "not text", "Comment character data");
    second.appendChild(first);
    assert(first.parentNode === second && document.body.lastChild === second, "Reparent updates both trees");
    let rejected = false;
    try { first.appendChild(second); } catch (_) { rejected = true; }
    assert(rejected && second.parentNode === document.body, "Cycle rejection leaves tree intact");
    second.removeChild(first);
    assert(first.parentNode === null && document.getElementById("first") === null, "Detached IDs excluded from document search");
    document.body.appendChild(first);
    first.textContent = "line\n\ttab\0é";
    assert(first.nodeName === "P" && first.id === "first", "textContent preserves element identity and attributes");
    assert(first.firstChild === node_7 && node_5.parentNode === null, "Replacement IDs and old node detachment");
    const added = document.createElement("span");
    assert(added === node_8, "Created node ID follows implicit text replacement allocation");
    added.id = "dynamic";
    added.appendChild(document.createTextNode("dynamic text"));
    first.appendChild(added);
    assert(document.getElementById("dynamic") === added, "Created nodes participate in lookup");
    assert(first.textContent === "line\n\ttab\0édynamic text", "Subtree text updates immediately");
    const wire = __vios_takeMutations().split("\n").map(line => line.split("\t").map(decodeURIComponent));
    assert(wire.some(row => row[0] === "X" && row[2] === "line\n\ttab\0é"), "Mutation string round-trip");
    assert(wire.filter(row => row[0] === "C").map(row => row[1]).join(",") === "8,9", "No duplicate CreateNode for SetText allocation");
    let calls = 0;
    first.addEventListener("click", function (event) {
        assert(event.target === first && event.currentTarget === first && event.clientX === 12, "Real event target and coordinates");
        calls++;
    });
    __vios_sync(snapshot(), "Resynced");
    assert(document.getElementById("first") === first, "Resync retains wrappers");
    __vios_dispatch(3, "click", 12, 34, null);
    assert(calls === 1, "Resync retains listeners");
    assert(document.title === "Resynced", "Snapshot title synchronized");
    rejected = false;
    try { first.appendChild(added); } catch (_) { rejected = true; }
    assert(rejected, "Stale nodes rejected after snapshot removes their IDs");
    const large = snapshot();
    for (let id = large.length; id <= 300; id++) large.push([id, 1, "div", "", [["id", "n" + id]], null, null, null]);
    large[2][6] = 300; large[300][5] = 2;
    __vios_sync(large, "Large");
    assert(document.getElementById("n300") === node_300, "Nodes above old fixed 256 window");
})();
