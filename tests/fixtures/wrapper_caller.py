"""Planted fixture: atlas-specific tile-fetch wrapper around fetch_tile
(wrapper_callee.py). It shares vocabulary with fetch_tile (tile, fetch, api,
timeout, retries) by construction — it calls fetch_tile directly — while
having a different shape (a cache check and a normalization branch fetch_tile
doesn't have). Caller and callee must not be reported as competing.
"""

from wrapper_callee import fetch_tile

_ATLAS_CACHE = {}


def fetch_atlas_tile(tile_id, map_id, api_key, timeout=30, retries=3, use_cache=True):
    cache_key = (map_id, tile_id)
    if use_cache and cache_key in _ATLAS_CACHE:
        cached = _ATLAS_CACHE[cache_key]
        if cached.get("stale") is not True:
            return cached
    tile = fetch_tile(tile_id, map_id, api_key, timeout=timeout, retries=retries)
    normalized = {
        "tile_id": tile.get("tile_id", tile_id),
        "map_id": tile.get("map_id", map_id),
        "api_key": api_key,
        "timeout": timeout,
        "retries": retries,
        "atlas_tag": "tile",
    }
    _ATLAS_CACHE[cache_key] = normalized
    return normalized
