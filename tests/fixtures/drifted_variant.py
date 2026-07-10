"""Planted fixture: drifted variant of clone_original.py's record parser.

Same behavior (turn a payload into a list of record dicts) and the same for-loop
skeleton, but each field is pulled into a local before a single-line dict is
built. That is more drift than the near-miss (parse_records_v2, ~0.75): it
sits *below* theta_clone (~0.44 to the clone core, so it stays out of the tight
clone cluster) yet well *above* theta_family, so the family altitude reunites
it as a drifted member of the same family — the fourth file in the lineage.
"""


def gather_records(payload):
    records = []
    for item in payload.get("features", []):
        props = item.get("properties", {})
        if not props:
            continue
        rid = props.get("id")
        name = props.get("name")
        value = props.get("value")
        records.append({"id": rid, "name": name, "value": value})
    return records
