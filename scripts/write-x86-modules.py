"""Retired one-shot generator for the Phase 09 x86 HAL bootstrap.

The maintained modules live under ``hal/arch/x86/src/x86_64``. Regenerating
them from the historical templates would discard ACPI-derived APIC routing and
the pre-ACPI COM1 diagnostics required by the real-hardware lane.
"""

raise SystemExit(
    "write-x86-modules.py is retired; edit hal/arch/x86/src/x86_64 directly"
)
