#!/usr/bin/env python3
"""Executable, persistent, assertion-bearing witnesses for the private lab.

--list-json is the host protocol manifest. A prepare boot blocks only at its
specified LAB-CUT; the host must hard-stop the VM. Recover verifies a separately
fsynced pre-cut snapshot of committed owner state before replaying original keys.
No store/key is reset or repaired. Results describe this finite SQLite owner set,
not universal resource quiescence, DCO1 qualification or physical power loss.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import fcntl
import multiprocessing
from pathlib import Path
import resource
import sqlite3
import sys
import time

try:
    from .auth import canonical
    from .runtime import Lab, Store, Refusal, CAPS, PROTOCOL, digest, require
except ImportError:
    from auth import canonical
    from runtime import Lab, Store, Refusal, CAPS, PROTOCOL, digest, require

CRASH = [
    {"name": "reserve-before", "cut": "issuer.reserve:before_commit"},
    {"name": "reserve-reply-loss", "cut": "issuer.reserve:after_commit"},
    {"name": "publication-before", "cut": "destination.publish:before_commit"},
    {"name": "publication-reply-loss", "cut": "destination.publish:after_commit"},
    {"name": "partial-credit-reply-loss", "cut": "authority.credit:after_commit"},
    {"name": "fence-intent-before", "cut": "issuer.fence_started:before_commit"},
    {"name": "fence-intent-after", "cut": "issuer.fence_started:after_commit"},
    {"name": "acceptance-before", "cut": "authority.accept:before_commit"},
    {"name": "acceptance-reply-loss", "cut": "authority.accept:after_commit"},
    {"name": "settlement-reply-loss", "cut": "issuer.settle:after_commit"},
    {"name": "successor-reply-loss", "cut": "authority.adopt:after_commit"},
    {"name": "release-before", "cut": "authority.release:before_commit"},
    {"name": "release-reply-loss", "cut": "authority.release:after_commit"},
]
MANIFEST = {"normal": ["sequential", "refusal-controls", "abort-before-fence", "reserved-tail", "b4-unknown", "serializer-race", "replay-corruption", "adoption-capacity"], "crash": CRASH}


def emit(kind, body):
    encoded = canonical(body)
    require(len(encoded) < 65536, "terminal evidence capacity")
    print(kind + " " + encoded.decode("ascii"), flush=True)


def racing_publication(root, scope, receipt, connection):
    lab = Lab(Path(root), scope)
    def paused(label):
        if label == "destination.publish:before_commit":
            connection.send("holding-central-gate")
            require(connection.recv() == "commit", "race controller")
    lab.stores["destination"].fault = paused
    try:
        result = lab.call("destination", "publish", "race.publish",
                          {"owner": "issuer", "obligation": "O", "epoch": 1, "writer": "issuer", "proof": receipt},
                          role="destination")
        connection.send(result)
    finally:
        lab.close()


def snapshot(root):
    """Read committed state through independent connections, even before COMMIT."""
    result = {}
    for owner in ("issuer", "authority", "destination"):
        db = sqlite3.connect("file:" + str(root / (owner + ".sqlite")) + "?mode=rw", uri=True)
        try:
            state = json.loads(db.execute("SELECT body FROM state WHERE singleton=1").fetchone()[0])
            rows = db.execute("SELECT key,request,result FROM requests ORDER BY key").fetchall()
            result[owner] = {"state": state,
                            "keys": [[key, hashlib.sha256(request).hexdigest(), hashlib.sha256(receipt).hexdigest()]
                                     for key, request, receipt in rows],
                            "resources": [list(row) for row in db.execute("SELECT obligation,length(payload),slots,closed,generation,hex(payload) FROM resources ORDER BY obligation").fetchall()]}
        finally:
            db.close()
    return result


def save_witness(root, label):
    committed = snapshot(root)
    # Payload is represented by a content digest in the witness, not copied out.
    for item in committed.values():
        for row in item["resources"]:
            row[-1] = hashlib.sha256(bytes.fromhex(row[-1])).hexdigest()
    witness = {"label": label, "committed": committed}
    path = root / "cut-witness.json"
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    with os.fdopen(fd, "wb") as handle:
        handle.write(canonical(witness))
        handle.flush()
        os.fsync(handle.fileno())
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    return witness


def check_recovery(root, label):
    witness = json.loads((root / "cut-witness.json").read_bytes())
    require(witness["label"] == label, "wrong retained cut label")
    actual = snapshot(root)
    for item in actual.values():
        for row in item["resources"]:
            row[-1] = hashlib.sha256(bytes.fromhex(row[-1])).hexdigest()
    require(actual == witness["committed"], "recovery changed committed state/key/resource identity")
    return {"label": label, "committed_digest": digest(actual), "exact_original_keys": True}


class Witness:
    def __init__(self, lab):
        self.lab, self.checks = lab, []
        self.first_calls = 0

    def check(self, condition, message):
        require(condition, message)
        self.checks.append(message)

    def rejects(self, action, message):
        before = snapshot(self.lab.root)
        try:
            action()
        except (ValueError, sqlite3.Error):
            self.check(snapshot(self.lab.root) == before, message + ": no effect")
            return
        raise AssertionError(message + ": unexpectedly admitted")

    def call(self, owner, operation, key, args, role=None):
        store = self.lab.stores[owner]
        existing = store.lookup(key)
        result = self.lab.call(owner, operation, key, args, role=role)
        self.check(store.lookup(key) == result, key + ": durable same-key result")
        self.check(existing is None or result == existing, key + ": original transaction identity")
        if existing is None:
            self.first_calls += 1
        return result

    def dest(self, operation, key, proof, owner="issuer", epoch=1, writer="issuer"):
        return self.call("destination", operation, key,
                         {"owner": owner, "obligation": "O", "epoch": epoch,
                          "writer": writer, "proof": proof}, "destination")

    def sequential(self, reserved_tail=False, inherited_unknown=False, adoption_capacity=False):
        l = self.lab
        reserve_args = {"epoch": 1, "obligation": "O", "vector": [100, 10], "create_key": "create-O"}
        reserve = self.call("issuer", "reserve", "reserve-O", reserve_args, "issuer")
        self.check(l.result(reserve, "issuer")["status"] == "Unknown", "reserve retains Unknown")
        self.rejects(lambda: l.call("issuer", "reserve", "reserve-O", {**reserve_args, "vector": [101, 10]}, role="issuer"),
                     "sequential changed-key replay refused")
        forged = copy.deepcopy(reserve)
        forged["body"]["value"]["vector"] = [0, 0]
        self.rejects(lambda: self.dest("publish", "forged-publication", forged),
                     "sequential unauthentic admission refused")
        self.rejects(lambda: l.call("issuer", "reserve", "oversized-budget", {**reserve_args, "obligation": "oversized", "vector": [32769, 1]}, role="issuer"),
                     "sequential finite capacity refuses before effect")
        missing = l.root / "never-initialized"
        self.rejects(lambda: Store(missing, "issuer", l.keys, l.scope),
                     "sequential missing store refuses empty genesis")
        self.check(not missing.exists(), "sequential recovery refusal creates no store")
        publication = self.dest("publish", "publish-O", reserve)
        self.check(l.result(publication, "destination")["vector"] == [100, 10], "persistent BLOB/slots allocated")
        if not inherited_unknown:
            resolved = self.call("issuer", "resolve", "resolve-O", {"epoch": 1, "obligation": "O", "proof": publication}, "issuer")
            self.check(l.result(resolved, "issuer")["status"] == "Applied", "same-key lookup resolves publication Unknown")
        self.rejects(lambda: l.call("issuer", "release", "issuer-early-refund", {"epoch": 1, "obligation": "O", "proof": publication}, role="issuer"),
                     "authentic issuer command and publication cannot refund precommit")
        self.rejects(lambda: l.call("issuer", "resize", "issuer-resize", {"epoch": 1, "obligation": "O", "resize": "forbidden", "target": [140, 6]}, role="issuer"),
                     "issuer cannot perform ordinary resize before transfer")
        self.rejects(lambda: l.call("issuer", "admission", "issuer-admission", {"epoch": 1, "obligation": "O"}, role="issuer"),
                     "issuer cannot issue ordinary Authority admission")
        self.rejects(lambda: l.call("issuer", "recovery_fence", "issuer-recovery-bypass", {"old_epoch": 1, "new_epoch": 2, "writer": "successor", "adoption": "bypass"}, role="recovery"),
                     "issuer cannot bypass bootstrap with fixed-home recovery")
        prepared = self.call("issuer", "prepare", "A.prepare", {"epoch": 1, "adoption": "A"}, "issuer")
        self.rejects(lambda: l.call("issuer", "audit", "frozen-audit", {"epoch": 1}, role="issuer"),
                     "frozen source cannot append retained audit after manifest freeze")
        ready = self.call("authority", "ready", "A.ready", {"proof": prepared}, "authority")
        self.check(l.result(ready, "authority")["spendable"] is False, "B2 ready is nonspendable")
        self.rejects(lambda: l.call("authority", "reserve", "ready-spend", {"epoch": 1, "obligation": "illegal", "vector": [1, 1], "create_key": "illegal"}, role="authority"), "ready cannot mint budget")
        fenced = self.call("issuer", "fence_started", "A.fence", {"adoption": "A", "proof": ready}, "issuer")
        self.rejects(lambda: l.call("issuer", "abort", "late-abort", {"adoption": "A"}, role="issuer"), "FenceStarted excludes abort")
        destination_fence = self.call("destination", "fence", "A.destination-fence", {"owner": "issuer", "proof": fenced}, "destination")
        assigned = self.call("issuer", "assign", "A.assign", {"adoption": "A", "proof": destination_fence}, "issuer")
        accepted = self.call("authority", "accept", "A.accept", {"adoption": "A", "proof": assigned}, "authority")
        self.check(l.result(accepted, "authority")["phase"] == "Accepted", "B4 establishes accepted sole custody")
        self.check(l.state("issuer")["charged"] == [100, 10], "B4 retains nonspendable source debit origin")
        self.check(l.effective_custody()[0] == "authority", "only accepted destination is effective custodian")
        self.rejects(lambda: l.call("issuer", "reserve", "old-source-spend", reserve_args, role="issuer"), "old source cannot spend after B4")
        if l.stores["authority"].lookup("bootstrap.activate") is None:
            self.rejects(lambda: l.call("authority", "reserve", "preactivation-spend", {"epoch": 2, "obligation": "premature", "vector": [1, 1], "create_key": "premature"}, role="authority"),
                         "B4 alone cannot activate fresh mutation")
        if inherited_unknown and l.stores["authority"].lookup("bootstrap.resolve") is None:
            self.rejects(lambda: l.call("authority", "activate", "premature-activation", {"epoch": 2}, role="authority"),
                         "inherited Unknown blocks activation without refund")
            self.check(l.state("authority")["charged"] == [100, 10], "inherited Unknown retains original complete charge")
        if inherited_unknown:
            self.call("authority", "resolve", "bootstrap.resolve", {"epoch": 2, "obligation": "O", "proof": publication}, "authority")
        self.call("authority", "activate", "bootstrap.activate", {"epoch": 2}, "authority")
        settled = self.call("issuer", "settle", "A.settle", {"adoption": "A", "proof": accepted}, "issuer")
        self.rejects(lambda: l.call("issuer", "audit", "redirect-audit", {"epoch": 1}, role="issuer"),
                     "redirect source cannot append retained audit")
        self.check(l.result(settled, "issuer")["phase"] == "Settled" and l.state("issuer")["charged"] == [100, 10], "B5 settlement gives no credit")
        self.rejects(lambda: l.call("authority", "release", "incomplete-closure", {"epoch": 2, "obligation": "O", "proof": publication}, role="authority"),
                     "publication outcome alone cannot authorize fixed-home final credit")
        resize = self.call("authority", "resize", "resize-1", {"epoch": 2, "obligation": "O", "resize": "R1", "target": [140, 6]}, "authority")
        self.check(l.result(resize, "authority")["vector"] == [140, 10], "mixed vector reserves growth without netting shrink")
        self.rejects(lambda: self.dest("resize_apply", "wrong-kind-resize", accepted, "authority", 2, "authority"),
                     "non-resize result cannot authorize resize outcome")
        self.rejects(lambda: l.call("authority", "admission", "pending-resize-admission", {"epoch": 2, "obligation": "O"}, role="authority"),
                     "pending resize cannot mint substitute effect admission")
        applied = self.dest("resize_apply", "apply-R1", resize, "authority", 2, "authority")
        self.rejects(lambda: l.call("issuer", "credit", "issuer-shrink-credit", {"epoch": 1, "obligation": "O", "proof": applied, "installment": [0, 2]}, role="issuer"),
                     "authentic issuer command and applied outcome cannot issue shrink credit")
        partial = self.call("authority", "credit", "credit-R1-a", {"epoch": 2, "obligation": "O", "proof": applied, "installment": [0, 2]}, "authority")
        self.check(l.result(partial, "authority")["vector"] == [140, 8], "first partial shrink installment")
        final = self.call("authority", "credit", "credit-R1-b", {"epoch": 2, "obligation": "O", "proof": applied, "installment": [0, 2]}, "authority")
        self.check(l.result(final, "authority")["vector"] == [140, 6], "second partial shrink installment")
        cancel_resize = self.call("authority", "resize", "resize-2", {"epoch": 2, "obligation": "O", "resize": "R2", "target": [180, 4]}, "authority")
        self.check(l.result(cancel_resize, "authority")["vector"] == [180, 6], "Unknown cancellation remains fully charged")
        noapp = self.dest("noapp", "outcome-R2", cancel_resize, "authority", 2, "authority")
        # Both a changed transport request and a different transport key must lose
        # against the same permanent semantic resize outcome.
        if l.state("authority")["epoch"] == 2 and l.state("authority")["obligations"]["O"]["resize"] is not None:
            self.rejects(lambda: self.dest("resize_apply", "outcome-R2", cancel_resize, "authority", 2, "authority"), "noapp original key cannot apply")
            self.rejects(lambda: self.dest("resize_apply", "late-R2", cancel_resize, "authority", 2, "authority"), "noapp semantic key cannot apply")
        self.rejects(lambda: l.call("issuer", "cancel", "issuer-noapp-credit", {"epoch": 1, "obligation": "O", "proof": noapp}, role="issuer"),
                     "authentic issuer command and noapp closure cannot issue cancellation credit")
        cancelled = self.call("authority", "cancel", "cancel-R2", {"epoch": 2, "obligation": "O", "proof": noapp}, "authority")
        self.check(l.result(cancelled, "authority")["vector"] == [140, 6], "proven-noapp credits growth exactly once")
        prior_authority_admission = self.call("authority", "close_intent", "pre-S.admission", {"epoch": 2, "obligation": "O"}, "authority")
        recovery = self.call("authority", "recovery_fence", "S.fence", {"old_epoch": 2, "new_epoch": 3, "writer": "successor", "adoption": "S"}, "recovery")
        self.rejects(lambda: l.call("authority", "audit", "recovery-audit", {"epoch": 2}, role="authority"),
                     "recovery gate excludes old-writer retained audit")
        successor_fence = self.call("destination", "fence", "S.destination-fence", {"owner": "authority", "proof": recovery}, "destination")
        adopted = self.call("authority", "adopt", "S.adopt", {"adoption": "S", "proof": successor_fence}, "successor")
        self.check(l.result(adopted, "authority")["no_credit"] is True, "same-ledger succession preserves charge and origin")
        self.rejects(lambda: l.call("authority", "audit", "stale-audit", {"epoch": 2}, role="successor"),
                     "current successor cannot append stale-epoch retained audit")
        self.rejects(lambda: l.call("authority", "recovery_fence", "S.reused", {"old_epoch": 3, "new_epoch": 4, "writer": "successor", "adoption": "S"}, role="recovery"),
                     "semantic successor adoption identity cannot be reused")
        self.rejects(lambda: l.call("authority", "reserve", "stale-authority", {"epoch": 2, "obligation": "stale", "vector": [1, 1], "create_key": "stale"}, role="authority"), "stale ledger writer refused")
        self.rejects(lambda: self.dest("close", "stale-destination", prior_authority_admission, "authority", 2, "authority"), "stale destination admission refused")
        self.rejects(lambda: self.dest("close", "stale-proof-new-epoch", prior_authority_admission, "authority", 3, "successor"),
                     "new command epoch cannot launder old admission proof")
        admission = self.call("authority", "close_intent", "close.admission", {"epoch": 3, "obligation": "O"}, "successor")
        self.rejects(lambda: l.call("authority", "resize", "resize-after-closing", {"epoch": 3, "obligation": "O", "resize": "late", "target": [200, 6]}, role="successor"),
                     "durable Closing intent excludes growth before destination close")
        if reserved_tail:
            store = l.stores["authority"]
            cutoff = CAPS["requests_per_store"] - CAPS["reserved_finish_slots"]
            while store.db.execute("SELECT count(*) FROM requests").fetchone()[0] < cutoff:
                index = store.state()["revision"]
                l.call("authority", "audit", "retained-audit-" + str(index), {"epoch": 3}, role="successor")
                self.first_calls += 1
            self.check(store.db.execute("SELECT count(*) FROM requests").fetchone()[0] == cutoff,
                       "legitimate retained audits reach actual fresh cutoff")
            self.rejects(lambda: l.call("authority", "reserve", "tail-fresh-refusal", {"epoch": 3, "obligation": "tail-new", "vector": [1, 1], "create_key": "tail-new"}, role="successor"),
                         "fresh admission refuses at actual reserved-tail boundary")
        closure = self.dest("close", "close-O", admission, "authority", 3, "successor")
        self.check(l.result(closure, "destination")["quiescent"] is True, "complete finite resource owner closure")
        self.rejects(lambda: l.call("authority", "resize", "resize-after-close", {"epoch": 3, "obligation": "O", "resize": "late", "target": [200, 6]}, role="successor"),
                     "closed destination cannot be stranded by later growth")
        self.rejects(lambda: l.call("issuer", "release", "issuer-final-refund", {"epoch": 1, "obligation": "O", "proof": closure}, role="issuer"),
                     "authentic issuer command and complete closure cannot refund consumed bootstrap debit")
        release_args = {"epoch": 3, "obligation": "O", "proof": closure}
        release = self.call("authority", "release", "release-O", release_args, "successor")
        if reserved_tail:
            self.check(l.stores["authority"].db.execute("SELECT count(*) FROM requests").fetchone()[0] == cutoff + 1,
                       "original pending release commits inside reserved finish tail")
        before = snapshot(l.root)
        self.check(l.call("authority", "release", "release-O", release_args, role="successor") == release,
                   "lost release reply returns original signed receipt")
        self.check(snapshot(l.root) == before, "release retry cannot credit twice")
        self.check(l.state("authority")["charged"] == [0, 0] and l.state("authority")["obligations"]["O"]["status"] == "Released", "fixed-home final release credits once")
        self.rejects(lambda: self.dest("publish", "late-create", reserve), "closed create never republishes")
        self.check(l.state("issuer")["charged"] == [100, 10], "settlement never restores bootstrap envelope")
        if adoption_capacity:
            while len(l.state("authority")["adoptions"]) < CAPS["adoptions_per_store"]:
                state = l.state("authority")
                epoch = state["epoch"]
                aid = "capacity-" + str(len(state["adoptions"]))
                recovery = self.call("authority", "recovery_fence", aid + ".fence",
                                     {"old_epoch": epoch, "new_epoch": epoch + 1, "writer": "successor", "adoption": aid},
                                     "recovery")
                fence = self.call("destination", "fence", aid + ".destination-fence",
                                  {"owner": "authority", "proof": recovery}, "destination")
                self.call("authority", "adopt", aid + ".adopt",
                          {"adoption": aid, "proof": fence}, "successor")
            state = l.state("authority")
            self.check(len(state["adoptions"]) == CAPS["adoptions_per_store"],
                       "bootstrap and successor records jointly reach aggregate adoption cap")
            self.rejects(lambda: l.call("authority", "recovery_fence", "capacity-overflow.fence",
                                       {"old_epoch": state["epoch"], "new_epoch": state["epoch"] + 1,
                                        "writer": "successor", "adoption": "capacity-overflow"}, role="recovery"),
                         "next distinct adoption refuses before fencing at aggregate retained cap")
            self.check(l.state("authority")["charged"] == [0, 0] and l.state("authority")["gate"] == "open",
                       "adoption capacity refusal preserves accepted writer and released charge")
        return self.checks

    def serializer_race(self):
        l = self.lab
        reserve = self.call("issuer", "reserve", "race.reserve",
                            {"epoch": 1, "obligation": "O", "vector": [100, 10], "create_key": "race.create"}, "issuer")
        context = multiprocessing.get_context("spawn")
        receiver, sender = context.Pipe()
        process = context.Process(target=racing_publication, args=(str(l.root), l.scope, reserve, sender))
        process.start()
        sender.close()
        try:
            self.check(receiver.poll(30) and receiver.recv() == "holding-central-gate",
                       "destination pauses before commit while holding cross-store serializer")
            fd = os.open(l.root / "serializer.lock", os.O_RDWR)
            try:
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    self.check(True, "competing source fence cannot acquire mutation serializer")
                else:
                    raise AssertionError("destination effect lost centralized gate before commit")
            finally:
                os.close(fd)
            self.check(l.stores["destination"].lookup("race.publish") is None,
                       "paused uncommitted destination effect is not acknowledged")
            receiver.send("commit")
            self.check(receiver.poll(30), "destination publication commits before releasing gate")
            publication = receiver.recv()
            self.check(l.result(publication, "destination")["vector"] == [100, 10],
                       "serialized destination resource result is authentic")
            process.join(30)
            self.check(process.exitcode == 0, "publication worker exits cleanly")
            prepared = self.call("issuer", "prepare", "race.prepare", {"epoch": 1, "adoption": "race"}, "issuer")
            manifest = l.result(prepared, "issuer")["manifest"]
            self.check(manifest["destination_resources"] == [["O", 100, 10, 0, 1]],
                       "subsequent frozen source manifest includes serialized destination effect")
            self.rejects(lambda: self.dest("publish", "postfreeze-publication", reserve),
                         "source freeze excludes later destination admission")
        finally:
            receiver.close()
            if process.is_alive():
                process.terminate()
                process.join(10)
        return self.checks

    def replay_corruption(self):
        l = self.lab
        arguments = {"epoch": 1, "obligation": "O", "vector": [100, 10], "create_key": "create-O"}
        reserve = self.call("issuer", "reserve", "reserve-O", arguments, "issuer")
        other = self.call("issuer", "audit", "other-result", {"epoch": 1}, "issuer")
        for corruption in ("signature", "signed-substitution", "journal"):
            fixture = l.root / ("broken-replay-" + corruption)
            fixture.mkdir(mode=0o700)
            # Deliberately corrupt disposable copies, never repair/reset a real
            # scenario ledger. Damaged fixtures remain preserved as evidence.
            copied = sqlite3.connect(fixture / "issuer.sqlite")
            l.stores["issuer"].db.backup(copied)
            if corruption == "signature":
                damaged = copy.deepcopy(reserve)
                damaged["signature"] = "00" * 64
                copied.execute("UPDATE requests SET result=? WHERE key=?", (canonical(damaged), "reserve-O"))
            elif corruption == "signed-substitution":
                copied.execute("UPDATE requests SET result=? WHERE key=?", (canonical(other), "reserve-O"))
            else:
                copied.execute("UPDATE journal SET operation=? WHERE key=?", ("other-operation", "reserve-O"))
            copied.commit()
            copied.close()
            store = Store(fixture, "issuer", l.keys, l.scope)
            try:
                row = store.prior("reserve-O")
                body = json.loads(row[0])
                command = l.keys.sign("issuer", "command:reserve", body)
                try:
                    store.transact(command, "issuer", "reserve",
                                   lambda *_: (_ for _ in ()).throw(AssertionError("replay invoked mutation")))
                except ValueError:
                    self.check(store.prior("reserve-O") == row,
                               corruption + " replay refuses without changing damaged evidence")
                else:
                    raise AssertionError(corruption + " replay returned corrupt success")
            finally:
                store.close()
        self.check(l.stores["issuer"].lookup("reserve-O") == reserve,
                   "negative corruption fixtures leave original authenticated ledger intact")
        return self.checks

    def controls(self):
        l = self.lab
        args = {"epoch": 1, "obligation": "O", "vector": [100, 10], "create_key": "create-O"}
        receipt = self.call("issuer", "reserve", "reserve-O", args, "issuer")
        self.rejects(lambda: l.call("issuer", "reserve", "reserve-O", {**args, "vector": [101, 10]}, role="issuer"), "changed same-key request refused")
        self.rejects(lambda: l.call("issuer", "reserve", "foreign-CAS", {**args, "obligation": "other"}, role="issuer", expected=0), "stale CAS refused")
        self.rejects(lambda: l.call("issuer", "reserve", "over-budget", {**args, "obligation": "huge", "vector": [32769, 1]}, role="issuer"), "finite budget exhausted before effect")
        for bad_vector in ([18446744073709551616, 0], [-1, 0], [True, 0], [1]):
            self.rejects(lambda v=bad_vector: l.call("issuer", "reserve", "bad-u64", {**args, "obligation": "bad", "vector": v}, role="issuer"), "checked u64 vector refusal")
        self.rejects(lambda: l.call("issuer", "release", "proof-omitted", {"epoch": 1, "obligation": "O", "proof": {}}, role="issuer"), "missing release proof refused")
        tampered = copy.deepcopy(receipt)
        tampered["body"]["value"]["vector"] = [0, 0]
        self.rejects(lambda: self.dest("publish", "tampered-publication", tampered), "changed proof content refused")
        for field, value in (("scope", "other-scope"), ("purpose", "wrong-purpose"), ("role", "recovery"), ("key_pin", "00" * 32), ("roster_pin", "00" * 32)):
            bad = copy.deepcopy(receipt)
            bad[field] = value
            self.rejects(lambda proof=bad: self.dest("publish", "bad-fact", proof), "changed " + field + " refused")
        self.rejects(lambda: l.keys.verify(receipt, "authority", "ledger-result"), "wrong required producer refused")
        command = {"ledger": l.stores["issuer"].identity, "operation": "reserve", "role": "authority", "key": "wrong-role", "expected": l.state("issuer")["revision"], "args": args}
        wrong_role = l.keys.sign("authority", "command:reserve", command)
        self.rejects(lambda: l.call("issuer", "reserve", "wrong-role", args, role="authority", envelope=wrong_role), "authentic unauthorized command role refused")
        # Exactly fill the retained obligation bound. No row is evicted to admit
        # another request. Existing reserve/outcome keys remain replayable.
        for index in range(1, CAPS["obligations"]):
            self.call("issuer", "reserve", "bounded-" + str(index), {"epoch": 1, "obligation": "O" + str(index), "vector": [1, 1], "create_key": "K" + str(index)}, "issuer")
        self.rejects(lambda: l.call("issuer", "reserve", "capacity-refusal", {"epoch": 1, "obligation": "overflow", "vector": [1, 1], "create_key": "overflow"}, role="issuer"), "retained obligation capacity refuses without eviction")
        self.check(l.stores["issuer"].lookup("reserve-O") == receipt, "capacity refusal preserves original key")
        missing = l.root / "missing-owner-control"
        self.rejects(lambda: Store(missing, "issuer", l.keys, l.scope), "missing initialized store never creates genesis")
        self.check(not missing.exists(), "missing-store refusal creates no directory")
        # Explicit malformed fixture, not repair/reset of a real owner store.
        corrupt = l.root / "corrupt-owner-control"
        if not corrupt.exists():
            corrupt.mkdir(mode=0o700)
            fd = os.open(corrupt / "issuer.sqlite", os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
            with os.fdopen(fd, "wb") as handle:
                handle.write(b"not-a-sqlite-ledger")
                handle.flush()
                os.fsync(handle.fileno())
        self.rejects(lambda: Store(corrupt, "issuer", l.keys, l.scope), "corrupt store refuses instead of resetting")
        self.rejects(lambda: Store(l.root, "issuer", l.keys, "wrong-scope"), "wrong initialized ledger refuses")
        self.rejects(lambda: Lab(l.root, l.scope, initialize=True), "explicit initialization cannot overwrite existing namespace")
        self.check(l.state("issuer")["charged"] == [100 + CAPS["obligations"] - 1, 10 + CAPS["obligations"] - 1], "all Unknown obligations retain full charge")
        return self.checks

    def abort(self):
        l = self.lab
        self.call("issuer", "reserve", "reserve-O", {"epoch": 1, "obligation": "O", "vector": [100, 10], "create_key": "create-O"}, "issuer")
        prepared = self.call("issuer", "prepare", "A.prepare", {"epoch": 1, "adoption": "A"}, "issuer")
        ready = self.call("authority", "ready", "A.ready", {"proof": prepared}, "authority")
        aborted = self.call("issuer", "abort", "A.abort", {"adoption": "A"}, "issuer")
        self.call("authority", "abort_ready", "A.abort-ready", {"proof": aborted}, "authority")
        self.rejects(lambda: l.call("issuer", "fence_started", "late-fence", {"adoption": "A", "proof": ready}, role="issuer"), "Aborted excludes FenceStarted and fence IO")
        self.check(l.state("issuer")["gate"] == "open" and l.state("authority")["gate"] == "inactive", "abort only unfreezes original valid owner")
        self.check(l.state("issuer")["charged"] == [100, 10] and l.state("authority")["charged"] == [0, 0], "abort reclaims preparation not obligation charge")
        self.check(l.state("destination")["fences"] == {}, "abort performs no destination fence")
        return self.checks


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list-json", action="store_true")
    parser.add_argument("--data", type=Path)
    parser.add_argument("--scenario")
    parser.add_argument("--phase", choices=("run", "prepare", "recover"), default="run")
    args = parser.parse_args(argv)
    if args.list_json:
        print(canonical(MANIFEST).decode("ascii"))
        return 0
    parser.error("--data and --scenario are required") if args.data is None or args.scenario is None else None
    started = time.monotonic_ns()
    lab = None
    checks = []
    try:
        names = MANIFEST["normal"] + [entry["name"] for entry in CRASH]
        require(args.scenario in names, "unknown source-defined scenario")
        require(args.data.is_dir(), "data volume mount directory missing")
        target = next((entry["cut"] for entry in CRASH if entry["name"] == args.scenario), None)
        require((target is None and args.phase == "run") or (target is not None and args.phase in ("prepare", "recover")), "scenario/phase mismatch")
        root = args.data / args.scenario
        scope = "vmware-durable-charge-lab/" + args.scenario
        recovery = None
        if args.phase == "recover":
            # Open first: rollback recovery and initialized identity checks must
            # succeed before comparing the original committed cut snapshot.
            lab = Lab(root, scope)
            recovery = check_recovery(root, target)
            checks.append("hardstop recovery preserves exact committed state and original keys")
        else:
            require(not root.exists(), "existing namespace must be recovered, never erased/reinitialized")

            def cut(label):
                if label == target:
                    witness = save_witness(root, label)
                    emit("LAB-CUT", {"label": label, "committed_digest": digest(witness["committed"])})
                    while True:
                        time.sleep(3600)

            lab = Lab(root, scope, initialize=True, fault=cut if target else None)
        witness = Witness(lab)
        if args.scenario == "serializer-race":
            checks.extend(witness.serializer_race())
        elif args.scenario == "replay-corruption":
            checks.extend(witness.replay_corruption())
        elif args.scenario == "refusal-controls":
            checks.extend(witness.controls())
        elif args.scenario == "abort-before-fence":
            checks.extend(witness.abort())
        else:
            checks.extend(witness.sequential(reserved_tail=args.scenario == "reserved-tail",
                                             inherited_unknown=args.scenario == "b4-unknown",
                                             adoption_capacity=args.scenario == "adoption-capacity"))
        require(args.phase != "prepare", "prepare did not reach required real fault hook")
        measurements = lab.observations()
        measurements.update({"elapsed_ns": time.monotonic_ns() - started,
                             "max_rss_kib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                             "commits_this_boot": witness.first_calls})
        if recovery:
            measurements["recovery"] = recovery
        emit("LAB-RESULT", {"scenario": args.scenario, "phase": args.phase, "ok": True,
                            "checks": checks, "measurements": measurements,
                            "public_roster": lab.keys.public_roster(),
                            "protocol": PROTOCOL,
                            "limits": ["private JSON Ed25519 carrier; not DCO1/native G3", "same guest/root trust domain", "ordinary virtual-volume crash only; no independent antirollback floor", "closure covers only admitted lab SQLite BLOB/slot resource owner"]})
        return 0
    except Exception as exc:
        # Never print request/envelope/private-key material or exception payloads.
        emit("LAB-RESULT", {"scenario": args.scenario, "phase": args.phase, "ok": False,
                            "checks": checks, "error_type": type(exc).__name__,
                            "error": str(exc) if isinstance(exc, (Refusal, AssertionError)) else "guest scenario failed closed",
                            "public_roster": lab.keys.public_roster() if lab else {}})
        return 1
    finally:
        if lab:
            lab.close()


if __name__ == "__main__":
    sys.exit(main())
