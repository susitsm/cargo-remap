#!/usr/bin/env python3
"""Turn RESULT lines into a distro x check matrix and list the failures."""
import sys
rows = [l.rstrip("\n").split("|", 5) for l in open(sys.argv[1]) if l.startswith("RESULT|")]
distros, checks, cell, detail = [], [], {}, {}
for _, d, c, s, *rest in rows:
    if d not in distros: distros.append(d)
    if c not in checks: checks.append(c)
    key = (d, c)
    if cell.get(key) != "FAIL":       # a FAIL in either run wins
        cell[key] = s; detail[key] = rest[0] if rest else ""
UNSUPPORTED = {"alpine-busybox"}   # expected to fail: no GNU util-linux
XFAIL = set()
sym = {"PASS": "ok", "FAIL": "FAIL", "SKIP": "-"}
print("| check | " + " | ".join(distros) + " |"); print("|---|" + "---|" * len(distros))
for c in checks: print(f"| {c} | " + " | ".join(("xfail" if (d in UNSUPPORTED or (d, c) in XFAIL) and cell.get((d, c)) == "FAIL" else sym.get(cell.get((d, c), ""), "?")) for d in distros) + " |")
bad = [(d, c) for (d, c), s in cell.items() if s == "FAIL" and d not in UNSUPPORTED and (d, c) not in XFAIL]
if bad:
    print("\nFailures:")
    for d, c in bad: print(f"- {d}: {c}: {detail[(d, c)]}")
