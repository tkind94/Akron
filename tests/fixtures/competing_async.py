"""Planted fixture: async class-based proxy fetcher (competes with competing_sync.py)."""

import aiohttp


class ProxyFetcher:
    def __init__(self, proxy_url, timeout=30, retries=3):
        self.proxy_url = proxy_url
        self.client_timeout = aiohttp.ClientTimeout(total=timeout)
        self.retries = retries

    async def fetch_page(self, url):
        """Fetch a page through the proxy with retry."""
        async with aiohttp.ClientSession(timeout=self.client_timeout) as session:
            attempt = 0
            while attempt < self.retries:
                async with session.get(url, proxy=self.proxy_url) as response:
                    if response.status == 200:
                        return await response.text()
                attempt += 1
        raise ConnectionError("proxy fetch failed after retries")
