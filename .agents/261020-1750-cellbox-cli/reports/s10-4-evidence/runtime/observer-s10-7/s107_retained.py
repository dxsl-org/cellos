"""Single import point for the retained S10-4.17 stack (feed-hook observer + coordinated/recurring/amended layers).

Importing this module puts every retained archive on `sys.path` (done by the retained modules themselves), redirects the
retained module attributes that point at vanished `/tmp` trees (recorded in `REDIRECTS`) and exposes the layers:

    HO feed-hook observer  HS feed-hook surface  C coordinated  F run_atomic  G gui_temporal  X diagnostic  M common
    A step-amendment (retained_health)  R recurring (RecurringClient)  OBS runtime/observer.py  FS feed_static
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402

sys.path.insert(0, str(ENV.FEED_HOOK))
import hook_observer as HO  # noqa: E402
import hook_surface as HS  # noqa: E402
import feed_static as FS  # noqa: E402
import observer as OBS  # noqa: E402

G, C, F, X, M = HO.G, HO.C, HO.F, HO.X, HO.M
A, R = C.A, C.R

REDIRECTS = ENV.redirect_retained({'common': M, 'observer': OBS})
