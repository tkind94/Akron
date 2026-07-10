"""Planted fixture: identifier-renamed clone of clone_original.py."""


def extract_rows(body):
    rows = []
    for entry in body.get("results", []):
        attrs = entry.get("attributes", {})
        if not attrs:
            continue
        rows.append(
            {
                "key": attrs.get("key"),
                "label": attrs.get("label"),
                "amount": attrs.get("amount"),
            }
        )
    return rows
