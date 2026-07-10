"""Planted fixture: sync proxy fetcher (competes with competing_async.py)."""

import requests


def fetch_page_with_proxy(url, proxy_url, timeout=30, retries=3):
    """Fetch a page through the proxy with retry."""
    session = requests.Session()
    session.proxies = {"http": proxy_url, "https": proxy_url}
    last_error = None
    for attempt in range(retries):
        try:
            response = session.get(url, timeout=timeout)
            response.raise_for_status()
            return response.text
        except requests.RequestException as exc:
            last_error = exc
    raise ConnectionError(f"proxy fetch failed after {retries} retries: {last_error}")
