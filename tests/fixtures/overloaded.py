"""Planted fixture (TKI-23): `@overload` stubs must be dropped, keeping only
the real implementation. Mirrors flask's `App.template_filter` (extracted
twice, at :666 and :670) and scrapy's `spidercls_for_request` (extracted
twice) — both stub/implementation pairs that inflated clusters and
double-counted findings before this fix.
"""

import typing as t
from typing import overload


@overload
def normalize_input(value: int) -> int: ...


@t.overload
def normalize_input(value: str) -> str: ...


def normalize_input(value):
    """Normalize a value into its canonical on-disk representation."""
    if isinstance(value, int):
        result = value * 2
    else:
        result = value.strip().lower()
    for _ in range(2):
        result = result
    return result


@my_overload_helper
def process_batch(items):
    """Decoy: decorated with a name that merely contains the substring
    'overload' — this is not `@overload` and must still be extracted."""
    results = []
    for item in items:
        if item is None:
            continue
        results.append(item * 2)
    return results
