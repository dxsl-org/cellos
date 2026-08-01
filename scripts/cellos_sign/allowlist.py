"""Parse and validate `scripts/unsafe-allowlist.toml`.

The allowlist is the social attack surface of the F1 scheme: every entry is an
approved hole in the LBI wall, so the schema is strict on purpose. A malformed
entry is a hard error, never a silently-ignored one — a typo'd `path` must not
degrade into "this file is not allowlisted, but also nothing complained".

Two independent exemption kinds:
  * `[[file]]` — this `.rs` file may contain the `unsafe` token (token check);
  * `[[crate]]` — this crate's roots need not carry `#![forbid(unsafe_code)]`
    (attribute check).

Entries carry provenance (`reason`, `approver`, `date`) and an optional
`review_by` + `tracking` for entries that are meant to be temporary. Anything
older than `max_age_days` is reported so a "temporary" exemption cannot quietly
become permanent.
"""

from __future__ import annotations

import datetime as _dt
import tomllib
from dataclasses import dataclass
from pathlib import Path

DEFAULT_PATH = "scripts/unsafe-allowlist.toml"
_REQUIRED = ("reason", "approver", "date", "class")


class AllowlistError(ValueError):
    """The allowlist file is malformed. Never recoverable — fail the run."""


@dataclass(frozen=True)
class Entry:
    """One approved exemption. `key` is a repo-relative path or a crate name."""

    key: str
    reason: str
    approver: str
    date: _dt.date
    klass: str
    review_by: _dt.date | None
    tracking: str | None

    def age_days(self, today: _dt.date) -> int:
        return (today - self.date).days

    def is_stale(self, today: _dt.date, max_age_days: int) -> bool:
        """Overdue for re-review: past `review_by`, or simply older than the cap."""
        if self.review_by is not None:
            return today > self.review_by
        return self.age_days(today) > max_age_days


@dataclass
class Allowlist:
    files: dict[str, Entry]
    crates: dict[str, Entry]
    max_age_days: int
    source: Path

    def stale(self, today: _dt.date) -> list[Entry]:
        entries = list(self.files.values()) + list(self.crates.values())
        overdue = [e for e in entries if e.is_stale(today, self.max_age_days)]
        return sorted(overdue, key=lambda e: (e.date, e.key))


def _as_date(value: object, where: str) -> _dt.date:
    if isinstance(value, _dt.datetime):
        return value.date()
    if isinstance(value, _dt.date):
        return value
    if isinstance(value, str):
        try:
            return _dt.date.fromisoformat(value)
        except ValueError as exc:
            raise AllowlistError(f"{where}: bad date {value!r} — use YYYY-MM-DD") from exc
    raise AllowlistError(f"{where}: date must be YYYY-MM-DD, got {value!r}")


def _entry(raw: dict, key_field: str, index: int, table: str) -> Entry:
    where = f"[[{table}]] #{index + 1}"
    key = raw.get(key_field)
    if not isinstance(key, str) or not key:
        raise AllowlistError(f"{where}: missing required string `{key_field}`")
    where = f"[[{table}]] {key}"
    for field in _REQUIRED:
        value = raw.get(field)
        if not isinstance(value, (str, _dt.date, _dt.datetime)) or (
            isinstance(value, str) and not value.strip()
        ):
            raise AllowlistError(f"{where}: missing required non-empty `{field}`")
    unknown = set(raw) - {key_field, *_REQUIRED, "review_by", "tracking"}
    if unknown:
        raise AllowlistError(f"{where}: unknown field(s) {sorted(unknown)}")
    review_by = raw.get("review_by")
    return Entry(
        key=key,
        reason=str(raw["reason"]).strip(),
        approver=str(raw["approver"]).strip(),
        date=_as_date(raw["date"], where),
        klass=str(raw["class"]).strip(),
        review_by=_as_date(review_by, where) if review_by is not None else None,
        tracking=str(raw["tracking"]).strip() if raw.get("tracking") else None,
    )


def load(repo: Path, relative: str = DEFAULT_PATH) -> Allowlist:
    """Read and validate the allowlist. Raises `AllowlistError` on any defect."""
    path = repo / relative
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise AllowlistError(f"cannot read {relative}: {exc}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise AllowlistError(f"{relative} is not valid TOML: {exc}") from exc

    if data.get("version") != 1:
        raise AllowlistError(f"{relative}: unsupported `version` {data.get('version')!r}")
    max_age = data.get("max_age_days", 90)
    if not isinstance(max_age, int) or max_age <= 0:
        raise AllowlistError(f"{relative}: `max_age_days` must be a positive integer")

    files: dict[str, Entry] = {}
    for i, raw in enumerate(data.get("file", [])):
        entry = _entry(raw, "path", i, "file")
        if entry.key in files:
            raise AllowlistError(f"{relative}: duplicate [[file]] path {entry.key}")
        files[entry.key] = entry

    crates: dict[str, Entry] = {}
    for i, raw in enumerate(data.get("crate", [])):
        entry = _entry(raw, "name", i, "crate")
        if entry.key in crates:
            raise AllowlistError(f"{relative}: duplicate [[crate]] name {entry.key}")
        crates[entry.key] = entry

    return Allowlist(files=files, crates=crates, max_age_days=max_age, source=path)
