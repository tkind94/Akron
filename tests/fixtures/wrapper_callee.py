"""Planted fixture: base tile-fetch helper (wrapped by wrapper_caller.py).

Mirrors a wrapper/callee case graded on a private corpus: a wrapper in
another file calls this and shares its vocabulary by construction, which
must not be reported as a competing pattern (see callrel.rs).
"""

import httpx


def fetch_tile(tile_id, map_id, api_key, timeout=30, retries=3):
    """Fetch a tile resource from the tile API with retry."""
    headers = {"Authorization": f"Bearer {api_key}"}
    for attempt in range(retries):
        response = httpx.get(
            f"https://api.tiles.example/v1/maps/{map_id}/tiles/{tile_id}",
            headers=headers,
            timeout=timeout,
        )
        if response.status_code == 200:
            return response.json()
    raise ConnectionError(
        f"tile {tile_id} fetch failed for map {map_id} after {retries} retries"
    )
