"""S10-7.2 mapped-backed bytes per labelled region from the guest page-table walk.

This is NOT host RSS, a working set, or a physical-memory figure.  Under the eager SAS identity
mapping (stack.rs allocate comment; no demand-paging/swap path found by source reading) a page
that is present in the guest tables is backed by a frame, so present = resident BY CONSTRUCTION;
that is a source-derived statement, not an accessed/dirty-bit measurement.  A page that is not
present is flagged, never counted, and never an exception; an unreadable page-table page is a
fail-closed `GuestReadError`.
"""
import sys
from dataclasses import dataclass, field

sys.dont_write_bytecode = True
from guest_view import PAGE  # noqa: E402

KINDS = ('elf-segment', 'user-stack', 'kernel-stack', 'heap-arena')
SCOPE = 'guest present mapped backing, eager SAS identity mapping; NOT host RSS, NOT working set, NOT physical-memory'


@dataclass(frozen=True)
class Region:
    label: str
    kind: str
    va: int
    size: int
    extra: dict = field(default_factory=dict, compare=False)

    def __post_init__(self):
        if self.kind not in KINDS:
            raise ValueError(f'region kind {self.kind!r} not in {KINDS}')
        if not self.label or self.size < 0 or self.va < 0 or self.va + self.size > 1 << 64:
            raise ValueError(f'invalid region {self.label!r}: va={self.va:#x} size={self.size}')


def page_span(region):
    """Page-aligned VA range covering the region (start rounded down, end rounded up, as observer does)."""
    if region.size == 0:
        return range(0)
    return range(region.va & ~(PAGE - 1), (region.va + region.size + PAGE - 1) & ~(PAGE - 1), PAGE)


def _ranges(vas):
    out = []
    for va in vas:
        if out and out[-1][1] == va:
            out[-1][1] = va + PAGE
        else:
            out.append([va, va + PAGE])
    return out


def _count_region(view, region, include_pages):
    mapped, missing, frames, leaf_sizes, pages = 0, [], set(), {}, []
    span = page_span(region)
    for va in span:
        info = view.page_info(va)
        if include_pages:
            pages.append(info)
        if info['present']:
            mapped += 1
            frames.add(info['pa'])
            size = str(1 << info['leaf_shift'])
            leaf_sizes[size] = leaf_sizes.get(size, 0) + 1
        else:
            missing.append(va)
    total = len(span)
    result = {
        'label': region.label, 'kind': region.kind, 'va': region.va, 'size_bytes': region.size,
        'page_span': [span.start, span.stop] if total else [region.va, region.va],
        'pages': total, 'present_pages': mapped, 'not_present_pages': len(missing),
        'mapped_backed_bytes': mapped * PAGE, 'not_present_bytes': len(missing) * PAGE,
        'fully_mapped': not missing, 'distinct_frames': len(frames),
        'not_present_ranges': _ranges(missing), 'leaf_page_sizes': leaf_sizes, 'extra': dict(region.extra),
    }
    if include_pages:
        result['page_states'] = pages
    return result, frames


def count_regions(view, regions, *, include_pages=False):
    """Per-region mapped-backed bytes + union totals (overlapping VA pages / shared frames counted once)."""
    results, frames_by, spans = [], {}, {}
    for region in regions:
        result, frames = _count_region(view, region, include_pages)
        results.append(result)
        frames_by[region.label] = frames
        spans[region.label] = set(page_span(region))
    if len(frames_by) != len(results):
        raise ValueError('duplicate region labels')
    for result in results:
        mine = spans[result['label']]
        result['overlaps'] = {other: len(mine & theirs) for other, theirs in spans.items()
                              if other != result['label'] and mine & theirs}
    all_frames = set().union(*frames_by.values()) if frames_by else set()
    non_heap = set().union(*(frames_by[r.label] for r in regions if r.kind != 'heap-arena')) if regions else set()
    total = {
        'sum_of_region_mapped_backed_bytes': sum(r['mapped_backed_bytes'] for r in results),
        'distinct_frame_bytes': len(all_frames) * PAGE,
        'distinct_frame_bytes_excluding_heap_arena': len(non_heap) * PAGE,
        'not_present_pages': sum(r['not_present_pages'] for r in results),
        'note': 'sum counts pages shared by overlapping regions (e.g. heap arena inside ELF .bss, adjacent segments on one page) more than once; distinct_frame_bytes does not',
    }
    return {'scope': SCOPE, 'regions': results, 'total': total}
