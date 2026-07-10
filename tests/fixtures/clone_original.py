"""Planted fixture: original of a renamed clone pair (see clone_renamed.py)."""


def parse_records(payload):
    records = []
    for item in payload.get("features", []):
        props = item.get("properties", {})
        if not props:
            continue
        records.append(
            {
                "id": props.get("id"),
                "name": props.get("name"),
                "value": props.get("value"),
            }
        )
    return records
