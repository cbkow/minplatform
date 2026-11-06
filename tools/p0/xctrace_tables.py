#!/usr/bin/env python3
"""Parse `xctrace export --xpath` XML tables into lists of dicts, resolving id/ref value sharing.
Usage as a module: rows = table(trace_path, schema). Each row maps column mnemonic -> (value, fmt)."""
import re, subprocess, sys, xml.etree.ElementTree as ET

def table(trace, schema):
    xp = f'/trace-toc/run[@number="1"]/data/table[@schema="{schema}"]'
    xml = subprocess.run(["xcrun", "xctrace", "export", "--input", trace, "--xpath", xp],
                         capture_output=True, text=True).stdout
    root = ET.fromstring(xml)
    node = root.find("node")
    if node is None: return []
    cols = [c.find("mnemonic").text for c in node.find("schema").findall("col")]
    byid = {}
    rows = []
    for r in node.findall("row"):
        vals = []
        for el in r:
            if "ref" in el.attrib:
                vals.append(byid.get(el.attrib["ref"], (None, None)))
            else:
                v = (el.text, el.attrib.get("fmt"))
                if el.tag == "sentinel": v = (None, None)
                if "id" in el.attrib: byid[el.attrib["id"]] = v
                vals.append(v)
        rows.append(dict(zip(cols, vals)))
    return rows

def num(v):
    try: return float(v[0]) if v and v[0] is not None else None
    except ValueError: return None

if __name__ == "__main__":
    for row in table(sys.argv[1], sys.argv[2])[: int(sys.argv[3]) if len(sys.argv) > 3 else 5]:
        print({k: v[0] for k, v in row.items()})
