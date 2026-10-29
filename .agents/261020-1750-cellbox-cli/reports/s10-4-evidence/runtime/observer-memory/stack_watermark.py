"""S10-7.2 exact stack watermark from guest memory of a `test-hooks` measurement kernel.

Mirrors kernel/src/task/stack.rs (read 2026-10-29):
  * `STACK_WATERMARK_PATTERN: u8 = 0xA5`;
  * `test_hook_prime_watermark` writes that byte over the WHOLE usable range
    `[usable_start(), top)` (`usable_bytes() = pages * PAGE_SIZE`, guard pages excluded);
  * `test_hook_watermark_bytes` returns `usable - untouched_prefix`, where `untouched_prefix`
    is the length of the run of pattern bytes starting at `usable_start` and ending at the
    first byte that differs (stacks grow DOWN, so low addresses stay untouched longest).
`depth_bytes` here is exactly that quantity.  It is a LOWER BOUND of the true depth: a store
of the pattern value (or any store the guest later overwrites with 0xA5) is invisible.

Fail-closed: misaligned/empty/overflowing range, any page of the usable range that is not
present, any unreadable page, an `sp` outside `[start, top]`, and (when `verify_primed` proof
is absent) a fully-consumed range, which cannot be told from an unprimed stack.
"""
import sys

sys.dont_write_bytecode = True
from guest_view import PAGE, GuestUnmappedError  # noqa: E402

PATTERN = 0xA5  # kernel/src/task/stack.rs STACK_WATERMARK_PATTERN


class StackRangeError(ValueError):
    """The supplied stack description is not a valid usable range / sp."""


class PrimeVerificationError(Exception):
    """A pre-run primed-range proof failed: a byte differs from the sentinel."""

    def __init__(self, va, got, pattern):
        super().__init__(f'byte at {va:#x} is {got:#04x}, expected primed pattern {pattern:#04x}')
        self.va, self.got = va, got


def validate_range(start, top):
    if not (isinstance(start, int) and isinstance(top, int)):
        raise StackRangeError('start/top must be integers')
    if start % PAGE or top % PAGE:
        raise StackRangeError(f'usable range [{start:#x},{top:#x}) is not page-aligned (kernel usable_bytes = pages*PAGE)')
    if not 0 <= start < top <= 1 << 64:
        raise StackRangeError(f'usable range [{start:#x},{top:#x}) is empty, inverted or beyond 2^64')


def leading_pattern_run(data, pattern=PATTERN):
    """Pure mirror of the kernel's scan loop over `data`: length of the leading pattern run."""
    for index, byte in enumerate(data):
        if byte != pattern:
            return index
    return len(data)


def _require_all_mapped(view, start, top):
    for va in range(start, top, PAGE):
        if view.translate(va)[0] is None:
            raise GuestUnmappedError(va)


def verify_primed(view, start, limit, pattern=PATTERN):
    """Pre-run proof: every byte of `[start, limit)` equals the sentinel.

    Take it at a stop where the stack has not run yet (e.g. the existing Cellbox entry stop for
    the user stack, below the initial rsp page) and pass the result to `measure(primed_proof=)`.
    """
    if start % PAGE or limit % PAGE or not 0 <= start < limit:
        raise StackRangeError(f'primed range [{start:#x},{limit:#x}) invalid (page-aligned, non-empty required)')
    for va in range(start, limit, PAGE):
        page = view.read(va, PAGE)
        if page.count(pattern) != PAGE:
            bad = leading_pattern_run(page, pattern)
            raise PrimeVerificationError(va + bad, page[bad], pattern)
    return {'start': start, 'limit': limit, 'bytes_checked': limit - start, 'pattern': pattern}


def measure(view, start, top, *, pattern=PATTERN, primed_proof=None, sp=None):
    """Exact sentinel depth of the usable range `[start, top)`; raises on any fail-closed case."""
    validate_range(start, top)
    usable = top - start
    if sp is not None and not start <= sp <= top:
        raise StackRangeError(f'sp {sp:#x} is outside the usable range [{start:#x},{top:#x}]')
    if primed_proof is not None and (primed_proof['start'] != start or primed_proof['pattern'] != pattern
                                     or not start < primed_proof['limit'] <= top):
        raise StackRangeError(f'primed proof {primed_proof!r} does not describe this stack range/pattern')
    _require_all_mapped(view, start, top)
    prefix = 0
    for va in range(start, top, PAGE):
        page = view.read(va, PAGE)
        run = leading_pattern_run(page, pattern)
        prefix += run
        if run < PAGE:
            break
    depth = usable - prefix
    if prefix == usable:
        status = 'untouched'
    elif prefix == 0:
        status = 'saturated-proven' if primed_proof is not None else 'saturated-unproven'
    else:
        status = 'partial'
    result = {
        'usable_start': start, 'top': top, 'usable_bytes': usable, 'pattern': pattern,
        'untouched_prefix_bytes': prefix, 'depth_bytes': depth, 'depth_pages': -(-depth // PAGE),
        'status': status,
        'exact': primed_proof is not None,
        'depth_valid': status != 'saturated-unproven',
        'prime_evidence': 'pre-run-verified' if primed_proof is not None else ('in-band-prefix' if prefix else 'none'),
        'primed_proof': primed_proof,
        'semantics': 'mirror of kernel test_hook_watermark_bytes: usable - leading run of pattern bytes; lower bound of true depth',
    }
    if sp is not None:
        result['sp'] = sp
        result['sp_depth_bytes'] = top - sp
        result['watermark_covers_sp'] = depth >= top - sp
    return result
