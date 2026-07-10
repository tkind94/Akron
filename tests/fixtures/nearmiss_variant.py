"""Planted fixture: Type-3 near-miss of clone_original.py (two edited lines)."""


def parse_records_v2(payload):
    records = []
    for item in payload.get("features", []):
        props = item.get("properties", {})
        if not props:
            continue
        name = props.get("name")
        records.append(
            {
                "id": props.get("id"),
                "name": name.strip() if name else None,
                "value": props.get("value"),
            }
        )
    return records
