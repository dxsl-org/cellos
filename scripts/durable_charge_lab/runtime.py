"""Private finite SQLite ownership lab; not DCO1 or a native resource grant.

Each owner has its own DELETE/FULL SQLite database. The destination's resources
are real persistent SQLite BLOB/slot rows, not kernel resources. All roles share
one guest root trust domain. No independent floor or snapshot rollback defense.

All Lab.call mutation admission, proof checks, epoch selection and commit hooks
share a namespace flock across guest processes. This deliberately centralized
lab serializer is not evidence of a native distributed fencing implementation.
Retained audit operations are finite authenticated journal writes (not effects):
they can fill the 192-entry fresh cutoff, but cannot consume the 64-entry finish
tail. No journal/outcome eviction or counter reset creates additional capacity.
"""
from __future__ import annotations

import hashlib
import fcntl
import json
import os
from pathlib import Path
import sqlite3
from typing import Callable

try:
    from .auth import Keyring, canonical
except ImportError:
    from auth import Keyring, canonical

CAPS = {"dimensions": ["blob_bytes", "lab_slots"], "budget": [32768, 128],
        "u64_max": 18446744073709551615, "obligations": 8,
        "requests_per_store": 256, "journal_per_store": 256,
        "reserved_finish_slots": 64, "resizes_per_obligation": 2,
        "credit_installments": 2, "admissions_per_obligation": 2,
        "adoptions_per_store": 8, "database_pages": 4096,
        "page_bytes": 4096, "request_bytes": 32768, "resource_rows": 8}
PROTOCOL = "cellos-durable-charge-lab-sqlite-v1"


class Refusal(ValueError):
    pass


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def vector(value):
    if (not isinstance(value, list) or len(value) != 2 or
            any(type(x) is not int or x < 0 or x > CAPS["u64_max"] for x in value)):
        raise Refusal("invalid checked-u64 vector")
    return value[:]


def add(a, b):
    return vector([x + y for x, y in zip(vector(a), vector(b))])


def sub(a, b):
    if any(x < y for x, y in zip(vector(a), vector(b))):
        raise Refusal("vector underflow")
    return vector([x - y for x, y in zip(a, b)])


def text(value):
    if not isinstance(value, str) or not value or len(value) > 96 or not value.isascii():
        raise Refusal("invalid finite identity")
    return value


def require(condition, message):
    if not condition:
        raise Refusal(message)


class Store:
    """One serializer: request identity, CAS, state, resources and result commit."""
    def __init__(self, root: Path, name: str, keys: Keyring, scope: str, initialize=False,
                 fault: Callable[[str], None] | None = None):
        self.name, self.keys, self.scope, self.fault = name, keys, scope, fault
        self.path = root / (name + ".sqlite")
        self.identity = digest({"protocol": PROTOCOL, "scope": scope, "owner": name,
                                "roster": keys.public_roster()})
        if initialize:
            fd = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
            os.close(fd)
        require(self.path.is_file(), "missing initialized " + name + " store")
        try:
            self.db = sqlite3.connect("file:" + str(self.path) + "?mode=rw", uri=True,
                                      isolation_level=None, timeout=5)
            self.db.execute("PRAGMA foreign_keys=ON")
            require(self.db.execute("PRAGMA journal_mode=DELETE").fetchone()[0] == "delete",
                    "wrong SQLite journal")
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.execute("PRAGMA max_page_count=" + str(CAPS["database_pages"]))
            if initialize:
                self._initialize()
            require(self.db.execute("PRAGMA quick_check").fetchone()[0] == "ok",
                    "corrupt owner store")
            row = self.db.execute("SELECT identity,protocol,scope FROM initialized").fetchall()
            require(row == [(self.identity, PROTOCOL, scope)], "wrong initialized ledger identity")
            self.state()
        except (sqlite3.Error, OSError, ValueError) as exc:
            if hasattr(self, "db"):
                self.db.close()
            raise Refusal("owner store unavailable: " + name) from exc

    def _initialize(self):
        self.db.executescript("""
        BEGIN IMMEDIATE;
        CREATE TABLE initialized(identity TEXT PRIMARY KEY,protocol TEXT NOT NULL,scope TEXT NOT NULL);
        CREATE TABLE state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),revision INTEGER NOT NULL,body BLOB NOT NULL);
        CREATE TABLE requests(key TEXT PRIMARY KEY,request BLOB NOT NULL,result BLOB NOT NULL);
        CREATE TABLE journal(revision INTEGER PRIMARY KEY,key TEXT NOT NULL UNIQUE REFERENCES requests(key),operation TEXT NOT NULL);
        CREATE TABLE resources(obligation TEXT PRIMARY KEY,payload BLOB NOT NULL,slots INTEGER NOT NULL CHECK(slots>=0),closed INTEGER NOT NULL CHECK(closed IN (0,1)),generation INTEGER NOT NULL);
        COMMIT;
        """)
        initial = {"identity": self.identity, "revision": 0, "epoch": 1,
                   "writer": "issuer" if self.name == "issuer" else "authority",
                   "gate": "open" if self.name == "issuer" else "inactive",
                   "charged": [0, 0], "limit": CAPS["budget"], "obligations": {},
                   "adoptions": {}, "fences": {}, "outcomes": {}}
        self.db.execute("BEGIN IMMEDIATE")
        self.db.execute("INSERT INTO initialized VALUES(?,?,?)", (self.identity, PROTOCOL, self.scope))
        self.db.execute("INSERT INTO state VALUES(1,0,?)", (canonical(initial),))
        self.db.execute("COMMIT")

    def state(self):
        row = self.db.execute("SELECT revision,body FROM state WHERE singleton=1").fetchone()
        require(row is not None, "missing initialized state")
        state = json.loads(row[1])
        canonical(state)
        require(state["identity"] == self.identity and state["revision"] == row[0],
                "state identity/revision mismatch")
        vector(state["charged"])
        return state

    def prior(self, key):
        return self.db.execute("SELECT request,result FROM requests WHERE key=?", (text(key),)).fetchone()

    def lookup(self, key):
        row = self.prior(key)
        if row is None:
            return None
        result = json.loads(row[1])
        body = self.keys.verify(result, "destination" if self.name == "destination" else self.name,
                                "destination-result" if self.name == "destination" else "ledger-result")
        request = json.loads(row[0])
        journal = self.db.execute("SELECT revision,operation FROM journal WHERE key=?", (key,)).fetchall()
        require(body["ledger"] == self.identity == request["ledger"] and body["key"] == key == request["key"] and
                body["operation"] == request["operation"] and body["revision"] == request["expected"] + 1 and
                body["request_digest"] == hashlib.sha256(row[0]).hexdigest() and
                journal == [(body["revision"], body["operation"])], "corrupt replay binding/journal")
        return result

    def transact(self, envelope, role, operation, handler):
        body = self.keys.verify(envelope, role, "command:" + operation)
        require(set(body) == {"ledger", "key", "expected", "args", "operation", "role"}, "incomplete command")
        require(body["ledger"] == self.identity and body["operation"] == operation and body["role"] == role,
                "wrong command ledger/operation/role")
        key = text(body["key"])
        require(type(body["expected"]) is int and body["expected"] >= 0, "invalid CAS revision")
        encoded = canonical(body)
        require(len(encoded) <= CAPS["request_bytes"], "request capacity")
        self.db.execute("BEGIN IMMEDIATE")
        try:
            prior = self.prior(key)
            if prior:
                require(prior[0] == encoded, "same-key changed immutable request")
                result = self.lookup(key)
                self.db.execute("ROLLBACK")
                return result
            count = self.db.execute("SELECT count(*) FROM requests").fetchone()[0]
            require(count < CAPS["requests_per_store"], "retained request capacity")
            require(self.db.execute("SELECT count(*) FROM journal").fetchone()[0] <
                    CAPS["journal_per_store"], "retained journal capacity")
            fresh = {"reserve", "resize", "prepare", "ready", "recovery_fence", "admission", "publish", "audit"}
            if operation in fresh:
                require(count < CAPS["requests_per_store"] - CAPS["reserved_finish_slots"],
                        "reserved finish request/result/journal capacity")
            state = self.state()
            require(state["revision"] == body["expected"], "stale expected revision")
            value = handler(state, body["args"], role)
            old_revision = state["revision"]
            state["revision"] += 1
            signer = "destination" if self.name == "destination" else self.name
            purpose = "destination-result" if self.name == "destination" else "ledger-result"
            result = self.keys.sign(signer, purpose,
                                    {"ledger": self.identity, "key": key, "operation": operation,
                                     "revision": state["revision"], "request_digest": hashlib.sha256(encoded).hexdigest(),
                                     "value": value})
            changed = self.db.execute("UPDATE state SET revision=?,body=? WHERE singleton=1 AND revision=?",
                                      (state["revision"], canonical(state), old_revision)).rowcount
            require(changed == 1, "CAS conflict")
            self.db.execute("INSERT INTO requests VALUES(?,?,?)", (key, encoded, canonical(result)))
            self.db.execute("INSERT INTO journal VALUES(?,?,?)", (state["revision"], key, operation))
            if self.fault:
                self.fault(self.name + "." + operation + ":before_commit")
            self.db.execute("COMMIT")
        except BaseException:
            if self.db.in_transaction:
                self.db.execute("ROLLBACK")
            raise
        if self.fault:
            self.fault(self.name + "." + operation + ":after_commit")
        return result

    def close(self):
        self.db.close()


class Lab:
    def __init__(self, root: Path, scope: str, initialize=False, fault=None):
        self.root, self.scope = Path(root), scope
        if initialize:
            require(not self.root.exists(), "explicit initialization cannot overwrite namespace")
        if initialize:
            self.root.mkdir(mode=0o700, parents=False, exist_ok=False)
            self.keys = Keyring.provision(self.root, scope)
        else:
            require(self.root.is_dir(), "missing scenario namespace")
            self.keys = Keyring.open(self.root, scope)
        self.stores = {}
        try:
            for name in ("issuer", "authority", "destination"):
                self.stores[name] = Store(self.root, name, self.keys, scope, initialize, fault)
            if initialize:
                for directory in (self.root, self.root.parent):
                    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
                    try:
                        os.fsync(fd)
                    finally:
                        os.close(fd)
        except BaseException:
            self.close()
            raise

    def close(self):
        for store in self.stores.values():
            store.close()
        self.stores = {}

    def state(self, owner):
        return self.stores[owner].state()

    def result(self, proof, owner, operation=None):
        body = self.keys.verify(proof, owner,
                                "destination-result" if owner == "destination" else "ledger-result")
        require(body["ledger"] == self.stores[owner].identity, "foreign result ledger")
        if operation:
            require(body["operation"] == operation, "wrong result operation")
        # A signature alone is not a live gate or proof of local durable publication.
        require(self.stores[owner].lookup(body["key"]) == proof, "result not retained at owner")
        return body["value"]

    def call(self, owner, operation, key, args, role=None, expected=None, envelope=None):
        # One centralized lab gate across separate SQLite owners/processes. This
        # is not a native distributed fence or independent authority boundary.
        fd = os.open(self.root / "serializer.lock", os.O_CREAT | os.O_RDWR, 0o600)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX)
            store = self.stores[owner]
            role = role or ("destination" if owner == "destination" else store.state()["writer"])
            if envelope is None:
                prior = store.prior(key)
                revision = json.loads(prior[0])["expected"] if prior else store.state()["revision"]
                body = {"ledger": store.identity, "key": key, "operation": operation, "role": role,
                        "expected": revision if expected is None else expected, "args": args}
                envelope = self.keys.sign(role, "command:" + operation, body)
            return store.transact(envelope, role, operation,
                                  lambda state, arguments, signer: self._apply(owner, operation, state, arguments, signer))
        finally:
            os.close(fd)

    def _gate(self, state, args, role, reconciliation=False):
        require(state["gate"] == "open" or (reconciliation and state["gate"] == "reconcile"), "mutation gate closed")
        require(role == state["writer"] and args.get("epoch") == state["epoch"], "stale writer")

    def _charge(self, state, amount):
        new = add(state["charged"], amount)
        require(all(x <= y for x, y in zip(new, state["limit"])), "budget capacity")
        state["charged"] = new

    def _manifest(self, state):
        owner = next(store for store in self.stores.values() if store.identity == state["identity"])
        rows = owner.db.execute("SELECT key,request,result FROM requests ORDER BY key").fetchall()
        destination = self.stores["destination"]
        retained = [[key, hashlib.sha256(request).hexdigest(), hashlib.sha256(result).hexdigest()]
                    for key, request, result in rows]
        resources = [list(row) for row in destination.db.execute(
            "SELECT obligation,length(payload),slots,closed,generation FROM resources ORDER BY obligation").fetchall()]
        return {"source": state["identity"], "epoch": state["epoch"],
                "source_watermark": state["revision"], "retained_requests": retained,
                "destination_watermark": destination.state()["revision"],
                "destination_resources": resources, "destination_outcomes": destination.state()["outcomes"],
                "charged": state["charged"], "obligations": state["obligations"]}

    def _obligation(self, state, args):
        oid = text(args["obligation"])
        require(oid in state["obligations"], "unknown obligation")
        return oid, state["obligations"][oid]

    def _apply(self, owner, op, s, a, role):
        if owner == "destination":
            return self._destination(op, s, a, role)
        if op in ("resize", "credit", "cancel", "release", "admission", "close_intent", "resolve_resize", "recovery_fence", "adopt", "activate"):
            require(owner == "authority", "only fixed-home Authority may resize/admit/credit/release/recover")
        if op == "audit":
            self._gate(s, a, role)
            return {"state_digest": digest(s), "revision": s["revision"], "no_effect": True}
        if op == "activate":
            require(s["gate"] == "reconcile" and role == s["writer"] and a["epoch"] == s["epoch"], "activation gate")
            for record in s["adoptions"].values():
                if record["phase"] != "Accepted":
                    continue
                source = self.stores["issuer"]
                for key, request_hash, result_hash in record["manifest"]["retained_requests"]:
                    retained = source.prior(key)
                    require(retained is not None and hashlib.sha256(retained[0]).hexdigest() == request_hash and
                            hashlib.sha256(retained[1]).hexdigest() == result_hash, "incomplete inherited journal")
                    source.lookup(key)
            total = [0, 0]
            destination = self.stores["destination"]
            inherited_outcomes = {}
            for record in s["adoptions"].values():
                if record["phase"] == "Accepted":
                    inherited_outcomes.update(record["manifest"]["destination_outcomes"])
            require(all(destination.state()["outcomes"].get(key) == value for key, value in inherited_outcomes.items()),
                    "lost inherited destination outcome")
            require({row[0] for row in destination.db.execute("SELECT obligation FROM resources").fetchall()} ==
                    set(s["obligations"]), "incomplete inherited resource owner enumeration")
            for key, in destination.db.execute("SELECT key FROM requests").fetchall():
                destination.lookup(key)
            for oid, obligation in s["obligations"].items():
                require(obligation["status"] in ("Applied", "Released") and obligation["resize"] is None,
                        "inherited Unknown blocks activation")
                row = destination.db.execute("SELECT length(payload),slots,closed FROM resources WHERE obligation=?", (oid,)).fetchone()
                require(row is not None and [row[0], row[1]] == obligation["actual"] and
                        bool(row[2]) == (obligation["status"] == "Released"), "unmatched inherited resource outcome")
                total = add(total, obligation["vector"])
            require(total == s["charged"], "incomplete inherited charge reconstruction")
            s["gate"] = "open"
            return {"activated": True, "epoch": s["epoch"], "charged": total}
        if op == "reserve":
            self._gate(s, a, role)
            oid = text(a["obligation"])
            require(oid not in s["obligations"], "obligation already exists")
            require(len(s["obligations"]) < CAPS["obligations"], "obligation capacity")
            amount = vector(a["vector"])
            self._charge(s, amount)
            origin = {"ledger": s["identity"], "obligation": oid,
                      "debit_revision": s["revision"] + 1, "create_key": text(a["create_key"])}
            obligation = {"origin": origin, "vector": amount, "actual": [0, 0],
                          "status": "Unknown", "resize": None, "epoch": s["epoch"],
                          "writer": s["writer"], "version": 1,
                          "resize_history": [], "admission_count": 0}
            s["obligations"][oid] = obligation
            return {"obligation": oid, **obligation}
        if op in ("admission", "close_intent"):
            self._gate(s, a, role)
            oid, obligation = self._obligation(s, a)
            renewable = op == "close_intent" and obligation["status"] == "Closing" and obligation["epoch"] < s["epoch"]
            require((obligation["status"] == "Applied" or renewable) and obligation["resize"] is None,
                    "only settled allocation/current successor admits closure")
            require(obligation["admission_count"] < CAPS["admissions_per_obligation"], "finite admission capacity")
            obligation["admission_count"] += 1
            obligation["version"] += 1
            obligation["epoch"], obligation["writer"] = s["epoch"], s["writer"]
            if op == "close_intent":
                obligation["status"] = "Closing"
            return {"obligation": oid, **obligation, "admitted_epoch": s["epoch"]}
        if op in ("resolve", "resize", "credit", "cancel", "release", "resolve_resize"):
            self._gate(s, a, role, reconciliation=op == "resolve")
            oid, obligation = self._obligation(s, a)
            require(obligation["status"] != "Released", "released obligation")
            if op == "resize":
                require(obligation["status"] == "Applied" and obligation["resize"] is None,
                        "unresolved conflicting resize")
                require(len(obligation["resize_history"]) < CAPS["resizes_per_obligation"] and
                        text(a["resize"]) not in obligation["resize_history"], "finite/nonreused resize capacity")
                obligation["resize_history"].append(a["resize"])
                obligation["version"] += 1
                obligation["epoch"], obligation["writer"] = s["epoch"], s["writer"]
                target = vector(a["target"])
                before = obligation["actual"]
                peak = [max(x, y) for x, y in zip(before, target)]
                growth = sub(peak, before)
                self._charge(s, growth)
                obligation["resize"] = {"id": text(a["resize"]), "before": before,
                                         "target": target, "peak": peak, "credited": [0, 0],
                                         "growth": growth, "outcome": "Unknown", "installments": 0}
                obligation["vector"] = peak
                obligation["status"] = "Unknown"
                return {"obligation": oid, **obligation}
            fact = self.result(a["proof"], "destination")
            require(fact["obligation"] == oid and fact["origin"] == obligation["origin"],
                    "wrong outcome obligation/debit origin")
            if op == "resolve":
                require(obligation["status"] == "Unknown" and fact["kind"] == "Applied" and
                        fact["vector"] == obligation["vector"] and obligation["resize"] is None,
                        "wrong creation outcome")
                obligation["actual"] = fact["vector"]
                obligation["status"] = "Applied"
            elif op == "resolve_resize":
                pending = obligation["resize"]
                require(pending is not None and fact["resize"] == pending["id"] and
                        fact["kind"] == "Applied" and fact["vector"] == pending["target"] and
                        pending["peak"] == pending["target"], "resize requires shrink credit")
                obligation["actual"] = pending["target"]
                obligation["resize"] = None
                obligation["status"] = "Applied"
            elif op in ("credit", "cancel"):
                pending = obligation["resize"]
                require(pending is not None and fact["resize"] == pending["id"], "wrong resize identity")
                if op == "cancel":
                    require(fact["kind"] == "NoApplication" and fact["closed_key"] is True and
                            fact["vector"] == pending["before"], "missing proven no-application closure")
                    require(pending["credited"] == [0, 0], "resize already credited")
                    s["charged"] = sub(s["charged"], pending["growth"])
                    obligation["vector"] = pending["before"]
                    obligation["actual"] = pending["before"]
                    obligation["resize"] = None
                    obligation["status"] = "Applied"
                else:
                    require(fact["kind"] == "Applied" and fact["vector"] == pending["target"],
                            "resize not proven applied")
                    installment = vector(a["installment"])
                    remaining = sub(sub(pending["peak"], pending["target"]), pending["credited"])
                    sub(remaining, installment)
                    require(installment != [0, 0], "empty installment")
                    require(pending["installments"] < CAPS["credit_installments"], "finite installment capacity")
                    require(pending["installments"] + 1 < CAPS["credit_installments"] or installment == remaining,
                            "last admitted installment must finish shrink")
                    pending["installments"] += 1
                    s["charged"] = sub(s["charged"], installment)
                    obligation["vector"] = sub(obligation["vector"], installment)
                    pending["credited"] = add(pending["credited"], installment)
                    obligation["actual"] = pending["target"]
                    if installment == remaining:
                        obligation["resize"] = None
                        obligation["status"] = "Applied"
            else:
                require(obligation["status"] == "Closing" and obligation["resize"] is None,
                        "missing Closing intent/unresolved resize blocks release")
                require(fact["admission_version"] == obligation["version"] and
                        fact["admission_epoch"] == obligation["epoch"], "wrong close intent result")
                require(fact["kind"] == "Closed" and fact["complete_owner_set"] == ["lab-sqlite-resource-map"]
                        and fact["quiescent"] is True and fact["closed_key"] is True and
                        fact["vector"] == [0, 0], "incomplete closure/quiescence proof")
                s["charged"] = sub(s["charged"], obligation["vector"])
                obligation["vector"] = [0, 0]
                obligation["actual"] = [0, 0]
                obligation["status"] = "Released"
            return {"obligation": oid, **obligation}
        if op == "prepare":
            self._gate(s, a, role)
            require(owner == "issuer", "bootstrap source must be issuer")
            aid = text(a["adoption"])
            require(aid not in s["adoptions"] and len(s["adoptions"]) < CAPS["adoptions_per_store"],
                    "adoption capacity/identity")
            manifest = self._manifest(s)
            record = {"adoption": aid, "manifest": manifest, "digest": digest(manifest),
                      "target": self.stores["authority"].identity, "phase": "Prepared",
                      "transactions": {"prepare": aid + ".prepare", "ready": aid + ".ready",
                                       "fence": aid + ".fence", "assign": aid + ".assign",
                                       "accept": aid + ".accept", "settle": aid + ".settle"}}
            s["adoptions"][aid] = record
            s["gate"] = "frozen"
            return record
        if op == "ready":
            require(owner == "authority" and role == "authority" and s["gate"] == "inactive", "ready cannot activate")
            record = self.result(a["proof"], "issuer", "prepare")
            require(record["target"] == s["identity"] and len(s["adoptions"]) < CAPS["adoptions_per_store"],
                    "wrong/capacity prepared target")
            require(all(x <= y for x, y in zip(record["manifest"]["charged"], s["limit"])), "prepared capacity")
            require(record["adoption"] not in s["adoptions"], "duplicate adoption")
            s["adoptions"][record["adoption"]] = {**record, "phase": "Ready"}
            return {**record, "phase": "Ready", "spendable": False}
        if op in ("abort", "fence_started", "assign", "accept", "settle"):
            aid = text(a["adoption"])
            require(aid in s["adoptions"], "missing adoption")
            record = s["adoptions"][aid]
            if op == "abort":
                require(owner == "issuer" and role == "issuer" and record["phase"] == "Prepared",
                        "abort forbidden after FenceStarted")
                record["phase"] = "Aborted"
                s["gate"] = "open"
            elif op == "fence_started":
                require(owner == "issuer" and role == "issuer" and record["phase"] == "Prepared",
                        "invalid FenceStarted")
                ready = self.result(a["proof"], "authority", "ready")
                require(ready["digest"] == record["digest"] and ready["adoption"] == aid, "wrong ready")
                record["phase"] = "FenceStarted"
                s["gate"] = "fenced"
            elif op == "assign":
                require(owner == "issuer" and role == "issuer" and record["phase"] == "FenceStarted", "assignment phase")
                fence = self.result(a["proof"], "destination", "fence")
                require(fence["ledger"] == s["identity"] and fence["epoch"] == s["epoch"] + 1 and
                        fence["adoption"] == aid, "missing exact destination fence")
                record["phase"] = "Assigned"
                record["fence"] = a["proof"]
            elif op == "accept":
                require(owner == "authority" and role == "authority" and record["phase"] == "Ready", "acceptance phase")
                assigned = self.result(a["proof"], "issuer", "assign")
                require(assigned["adoption"] == aid and assigned["digest"] == record["digest"] and
                        assigned["target"] == s["identity"], "assignment binding")
                live = self.state("issuer")["adoptions"][aid]
                require(live["phase"] in ("Assigned", "Settled"), "assignment not current")
                self.result(assigned["fence"], "destination", "fence")
                require(s["charged"] == [0, 0] and not s["obligations"], "target already owns charge")
                s["obligations"] = json.loads(canonical(record["manifest"]["obligations"]))
                s["charged"] = record["manifest"]["charged"][:]
                s["epoch"] = record["manifest"]["epoch"] + 1
                s["gate"] = "reconcile"
                record["phase"] = "Accepted"
            else:
                require(owner == "issuer" and role == "issuer" and record["phase"] == "Assigned", "settlement phase")
                accepted = self.result(a["proof"], "authority", "accept")
                require(accepted["adoption"] == aid and accepted["digest"] == record["digest"], "wrong acceptance")
                record["phase"] = "Settled"
                s["gate"] = "redirect"
            return record
        if op == "abort_ready":
            require(owner == "authority" and role == "authority", "wrong abort target")
            source = self.result(a["proof"], "issuer", "abort")
            record = s["adoptions"][source["adoption"]]
            require(record["phase"] == "Ready" and record["digest"] == source["digest"], "abort binding")
            record["phase"] = "Aborted"
            return record
        if op == "recovery_fence":
            require(role == "recovery" and s["gate"] == "open" and a["old_epoch"] == s["epoch"], "recovery fence authority")
            require(a["new_epoch"] == s["epoch"] + 1 and a["writer"] == "successor", "nonregressing successor")
            aid = text(a["adoption"])
            require(aid not in s["adoptions"], "semantic adoption identity already retained")
            require(len(s["adoptions"]) < CAPS["adoptions_per_store"] and
                    s.get("recovery_count", 0) < CAPS["adoptions_per_store"],
                    "aggregate retained adoption capacity")
            s["recovery_count"] = s.get("recovery_count", 0) + 1
            s["gate"] = "recovery"
            s["recovery"] = {**a, "manifest": self._manifest(s)}
            s["adoptions"][aid] = {**s["recovery"], "phase": "RecoveryFenced"}
            return s["recovery"]
        if op == "adopt":
            require(role == "successor" and s["gate"] == "recovery", "successor not fenced")
            recovery = s["recovery"]
            fence = self.result(a["proof"], "destination", "fence")
            require(fence["ledger"] == s["identity"] and fence["epoch"] == recovery["new_epoch"] and
                    fence["adoption"] == recovery["adoption"], "successor destination gate incomplete")
            manifest = recovery["manifest"]
            require(a["adoption"] == recovery["adoption"] and s["epoch"] == manifest["epoch"] and
                    s["charged"] == manifest["charged"] and s["obligations"] == manifest["obligations"],
                    "incomplete reconstruction")
            current_keys = {row[0]: row[1:] for row in self._manifest(s)["retained_requests"]}
            require(all(current_keys.get(row[0]) == row[1:] for row in manifest["retained_requests"]),
                    "lost retained source outcome")
            s["writer"], s["epoch"], s["gate"] = "successor", recovery["new_epoch"], "open"
            s["adoptions"][recovery["adoption"]]["phase"] = "Adopted"
            return {"adoption": a["adoption"], "epoch": s["epoch"], "manifest": recovery["manifest"], "no_credit": True}
        raise Refusal("unsupported ledger operation")

    def _destination(self, op, s, a, role):
        require(role == "destination", "wrong destination principal")
        db = self.stores["destination"].db
        if op == "fence":
            source_owner = a["owner"]
            require(source_owner in ("issuer", "authority"), "invalid fence owner")
            source = self.result(a["proof"], source_owner)
            source_state = self.state(source_owner)
            if "new_epoch" in source:
                require(source_state["gate"] == "recovery" and source == source_state["recovery"], "stale recovery fence")
                epoch, aid = source["new_epoch"], source["adoption"]
            else:
                require(source["phase"] == "FenceStarted" and source_state["gate"] == "fenced", "no durable FenceStarted")
                epoch, aid = source["manifest"]["epoch"] + 1, source["adoption"]
            ledger = self.stores[source_owner].identity
            require(epoch > s["fences"].get(ledger, 0), "regressed fence")
            s["fences"][ledger] = epoch
            resources = [list(row) for row in db.execute(
                "SELECT obligation,length(payload),slots,closed,generation FROM resources ORDER BY obligation").fetchall()]
            retained = [[key, hashlib.sha256(request).hexdigest(), hashlib.sha256(result).hexdigest()]
                        for key, request, result in db.execute("SELECT key,request,result FROM requests ORDER BY key").fetchall()]
            return {"ledger": ledger, "epoch": epoch, "adoption": aid,
                    "watermark": s["revision"], "unresolved_retained": True,
                    "resources": resources, "outcomes": s["outcomes"], "retained_requests": retained}
        require(op in ("publish", "resize_apply", "noapp", "close"), "unsupported destination operation")
        owner = a["owner"]
        require(owner in ("issuer", "authority"), "invalid destination ledger")
        ledger = self.state(owner)
        expected_operation = {"publish": "reserve", "resize_apply": "resize", "noapp": "resize", "close": "close_intent"}[op]
        admission = self.result(a["proof"], owner, expected_operation)
        require(admission["obligation"] == a["obligation"], "wrong admission obligation")
        oid, obligation = self._obligation(ledger, a)
        self._gate(ledger, a, a["writer"])
        require(a["epoch"] >= s["fences"].get(ledger["identity"], 1), "stale destination epoch")
        require(admission["origin"] == obligation["origin"], "wrong admission debit origin")
        require(admission["epoch"] == a["epoch"] and admission["writer"] == a["writer"] and
                admission["version"] == obligation["version"] and
                all(admission.get(field) == value for field, value in obligation.items()),
                "stale or changed obligation admission snapshot")
        row = db.execute("SELECT length(payload),slots,closed,generation FROM resources WHERE obligation=?", (oid,)).fetchone()
        if op == "publish":
            require(row is None and obligation["status"] == "Unknown" and obligation["resize"] is None,
                    "closed or existing resource key")
            require(admission["vector"] == obligation["vector"], "changed reserve")
            require(db.execute("SELECT count(*) FROM resources").fetchone()[0] < CAPS["resource_rows"], "resource capacity")
            amount = vector(obligation["vector"])
            db.execute("INSERT INTO resources VALUES(?,zeroblob(?),?,0,1)", (oid, amount[0], amount[1]))
            kind, resize = "Applied", None
        else:
            require(row is not None and row[2] == 0, "resource missing/closed")
            if op == "close":
                require(obligation["resize"] is None and obligation["status"] == "Closing", "missing durable Closing intent")
                db.execute("UPDATE resources SET payload=zeroblob(0),slots=0,closed=1,generation=generation+1 WHERE obligation=?", (oid,))
                kind, resize, amount = "Closed", None, [0, 0]
            else:
                pending = obligation["resize"]
                require(pending is not None and admission["resize"] == pending, "wrong pending resize")
                require([row[0], row[1]] == pending["before"], "resource outcome already changed")
                resize = pending["id"]
                outcome_key = oid + ":" + resize
                require(outcome_key not in s["outcomes"], "resize key permanently resolved/closed")
                require(len(s["outcomes"]) < CAPS["requests_per_store"], "retained outcome capacity")
                s["outcomes"][outcome_key] = "Applied" if op == "resize_apply" else "NoApplication"
                amount = pending["target"] if op == "resize_apply" else pending["before"]
                if op == "resize_apply":
                    db.execute("UPDATE resources SET payload=zeroblob(?),slots=?,generation=generation+1 WHERE obligation=?",
                               (amount[0], amount[1], oid))
                # The durable semantic resize tombstone also rejects a delayed
                # apply under a different transport key after NoApplication.
                kind = "Applied" if op == "resize_apply" else "NoApplication"
        totals = db.execute("SELECT coalesce(sum(length(payload)),0),coalesce(sum(slots),0) FROM resources WHERE closed=0").fetchone()
        prior_highwater = s.get("resource_highwater", [0, 0])
        s["resource_highwater"] = [max(prior_highwater[index], totals[index]) for index in range(2)]
        return {"obligation": oid, "origin": obligation["origin"], "kind": kind,
                "vector": amount, "resize": resize, "closed_key": kind in ("Closed", "NoApplication"),
                "admission_version": obligation["version"], "admission_epoch": obligation["epoch"],
                "complete_owner_set": ["lab-sqlite-resource-map"], "quiescent": kind == "Closed",
                "resource_generation": 1 if row is None else row[3] + (kind != "NoApplication")}

    def effective_custody(self):
        issuer, authority = self.state("issuer"), self.state("authority")
        if any(record["phase"] == "Accepted" for record in authority["adoptions"].values()):
            return "authority", authority["charged"]
        return "issuer", issuer["charged"]

    def observations(self):
        result = {"caps": CAPS, "sqlite_version": sqlite3.sqlite_version,
                  "effective_custody": list(self.effective_custody()), "stores": {}}
        for name, store in self.stores.items():
            state = store.state()
            result["stores"][name] = {"identity": store.identity, "revision": state["revision"],
                "charged": state["charged"], "epoch": state["epoch"], "gate": state["gate"],
                "requests": store.db.execute("SELECT count(*) FROM requests").fetchone()[0],
                "journal": store.db.execute("SELECT count(*) FROM journal").fetchone()[0],
                "resources": [list(row) for row in store.db.execute("SELECT obligation,length(payload),slots,closed,generation FROM resources ORDER BY obligation").fetchall()],
                "resource_highwater": state.get("resource_highwater", [0, 0]),
                "page_count": store.db.execute("PRAGMA page_count").fetchone()[0],
                "file_bytes": store.path.stat().st_size,
                "synchronous": store.db.execute("PRAGMA synchronous").fetchone()[0],
                "journal_mode": store.db.execute("PRAGMA journal_mode").fetchone()[0],
                "foreign_keys": store.db.execute("PRAGMA foreign_keys").fetchone()[0]}
        return result
