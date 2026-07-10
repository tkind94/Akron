"""Planted fixture: builds a Thumbnail (ctor_callee.py) via a direct
constructor call `Thumbnail(...)` — syntactically identical to calling a bare
function named Thumbnail. Shares vocabulary with Thumbnail.__init__ (image,
width, height, quality, format, cache) by construction, while having a
different shape (a cache lookup/branch that __init__ doesn't have) — must
not be reported as competing (see callrel.rs's constructor-call handling).
"""

from ctor_callee import Thumbnail

_THUMBNAIL_CACHE = {}


def build_thumbnail(image_id, width, height, quality=85, image_format="jpeg", use_cache=True):
    cache_key = f"{image_id}:{width}x{height}:{image_format}"
    if use_cache and cache_key in _THUMBNAIL_CACHE:
        existing = _THUMBNAIL_CACHE[cache_key]
        if existing.quality >= quality:
            return existing
    thumb = Thumbnail(image_id, width, height, quality=quality, image_format=image_format)
    _THUMBNAIL_CACHE[cache_key] = thumb
    return thumb
