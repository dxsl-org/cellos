"""S10-8 measured surfaces: the retained S10-7 status-selecting surfaces with the pause-credit recovery waits in front.

MRO: CreditedRecoveryMixin -> (S10-7 status mixin) -> retained surface.  Only `wait` (for the headless 10 s
'new interrupt prompt' wait) and `gui_fresh` (desktop 10 s 'runtime new GUI prompt' wait) are overridden, each delegating
every other call to the retained method.  No candidate-visible behaviour, input, command or deadline other than those two
wait deadlines differs from S10-7.4.
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s108_credit as CREDIT  # noqa: E402
import s107_machine as MACH7  # noqa: E402


def surface_classes(hook_surface_cls):
    """(desktop, headless) measured surface classes; hook_surface_cls is `hook_surface.HookSurface`."""
    desktop7, headless7 = MACH7.surface_classes(hook_surface_cls)

    class CreditedDesktopSurface(CREDIT.CreditedRecoveryMixin, desktop7):
        pass

    class CreditedHeadlessSurface(CREDIT.CreditedRecoveryMixin, headless7):
        pass
    return CreditedDesktopSurface, CreditedHeadlessSurface
