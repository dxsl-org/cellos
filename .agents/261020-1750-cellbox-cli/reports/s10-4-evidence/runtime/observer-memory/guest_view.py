"""S10-7.2 host-side read-only view of a PAUSED guest address space.

Reuses the retained `observer.walk_page` / `observer.MASK` by import (nothing re-implemented):
translation is the observer's own 4-level walk over `read_physical(pa, n)` (retained
`RspClient.read_physical_memory` in a live run, a synthetic fixture in tests).  Every failure
is surfaced: an unreadable table/page raises `GuestReadError`; an unmapped page raises
`GuestUnmappedError` when bytes are required, and is reported as `present=False` when the
caller only asks for page state.  A `GuestView` caches page-table pages, so create ONE per
pause point and drop it; never reuse across a resume.  Never writes guest memory.
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
RUNTIME = Path(__file__).resolve().parent.parent
if str(RUNTIME) not in sys.path:
    sys.path.insert(0, str(RUNTIME))
from observer import MASK, walk_page  # noqa: E402  (retained S10-4 machinery, unchanged)

PAGE = 4096
_PTE_FLAGS = ((0, 'present'), (1, 'writable'), (2, 'user'), (5, 'accessed'), (6, 'dirty'), (63, 'no_execute'))


class GuestMemoryError(Exception):
    """Base class: guest memory could not be established; callers must fail closed."""


class GuestReadError(GuestMemoryError):
    """A physical/virtual read failed or returned a wrong-sized reply."""


class GuestUnmappedError(GuestMemoryError):
    """Bytes were required from a virtual page that is not present in the page tables."""

    def __init__(self, va):
        super().__init__(f'page {va:#x} is not present in the page tables')
        self.va = va


class GuestView:
    def __init__(self, cr3, read_physical, *, walk=walk_page):
        self.cr3 = cr3
        self._read_physical = read_physical
        self._walk = walk
        self._cache = {}

    def _checked_physical(self, pa, length):
        try:
            data = self._read_physical(pa, length)
        except Exception as error:  # noqa: BLE001 - every transport/fixture failure is fatal here
            raise GuestReadError(f'physical read {pa:#x}+{length} failed: {type(error).__name__}: {error}') from error
        if not isinstance(data, (bytes, bytearray)) or len(data) != length:
            raise GuestReadError(f'physical read {pa:#x}+{length} returned {len(data) if hasattr(data, "__len__") else type(data)!r} bytes')
        return bytes(data)

    def translate(self, va):
        """Return (physical address or None, witness list) via the retained walker."""
        try:
            return self._walk(va, self.cr3, self._checked_physical, self._cache)
        except GuestMemoryError:
            raise
        except Exception as error:  # noqa: BLE001
            raise GuestReadError(f'page-table walk for {va:#x} failed: {type(error).__name__}: {error}') from error

    def page_info(self, va):
        """State of the 4 KiB page containing `va` (present or not; never raises for absence)."""
        page = va & ~(PAGE - 1)
        pa, witness = self.translate(page)
        leaf = witness[-1]
        info = {'va': page, 'present': pa is not None, 'leaf_shift': leaf['shift'], 'leaf_pte': leaf['pte']}
        info['pa'] = (pa & ~(PAGE - 1)) if pa is not None else None
        info['flags'] = {name: bool(leaf['pte'] >> bit & 1) for bit, name in _PTE_FLAGS} if pa is not None else None
        return info

    def read(self, va, length):
        """Read `length` bytes at virtual `va`; page-by-page, raising on any absent page."""
        if length < 0:
            raise ValueError('negative length')
        out = bytearray()
        while len(out) < length:
            cur = va + len(out)
            pa, _ = self.translate(cur & ~(PAGE - 1))
            if pa is None:
                raise GuestUnmappedError(cur & ~(PAGE - 1))
            count = min(length - len(out), PAGE - (cur & (PAGE - 1)))
            out += self._checked_physical((pa & ~(PAGE - 1)) | (cur & (PAGE - 1)), count)
        return bytes(out)
