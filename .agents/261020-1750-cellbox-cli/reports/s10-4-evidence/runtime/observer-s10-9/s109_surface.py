"""S10-9 measured surfaces: the retained S10-7 status-selecting surfaces with the latency-recording recovery waits in front.

MRO: LatencyRecoveryMixin -> CreditedRecoveryMixin (S10-8, unchanged) -> (S10-7 status mixin) -> retained surface.  Only the wait
object created by `_credit_start` differs from S10-8; `wait` (headless 10 s 'new interrupt prompt') and `gui_fresh` (desktop 10 s
'runtime new GUI prompt') are the unchanged S10-8 methods.
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_credit as CREDIT  # noqa: E402
import s107_machine as MACH7  # noqa: E402


def surface_classes(hook_surface_cls):
    """(desktop, headless) measured surface classes; hook_surface_cls is `hook_surface.HookSurface`."""
    desktop7, headless7 = MACH7.surface_classes(hook_surface_cls)

    class LatencyDesktopSurface(CREDIT.LatencyRecoveryMixin, desktop7):
        pass

    class LatencyHeadlessSurface(CREDIT.LatencyRecoveryMixin, headless7):
        pass
    return LatencyDesktopSurface, LatencyHeadlessSurface
