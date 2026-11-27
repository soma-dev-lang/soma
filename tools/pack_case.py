#!/usr/bin/env python3
"""A published case study is served twice: as files to browse (site/cases/<name>/src)
and as an archive to download (<name>.tar.gz, with MANIFEST.txt and, when it has
one, index.json). Nothing kept the two together: Orbite's archive stayed nine days
behind its sources, and on it `soma verify prod.cell` failed while the page said OK.

    tools/pack_case.py orbite          rebuild the archive, the manifest and the index from src/
    tools/pack_case.py --check         every case: the archive holds exactly the files of src/

`build_site.py` runs the check: a case whose archive is not its sources fails the build.
"""
import hashlib
import io
import json
import os
import sys
import tarfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CASES = os.path.join(ROOT, "site", "cases")
SKIP = (".soma_cache", ".soma_data", ".DS_Store")


def sources(case):
    """{relative path: bytes} of site/cases/<case>/src."""
    src = os.path.join(CASES, case, "src")
    out = {}
    for d, dirs, files in os.walk(src):
        dirs[:] = sorted(x for x in dirs if x not in SKIP)
        for f in sorted(files):
            if f in SKIP:
                continue
            p = os.path.join(d, f)
            out[os.path.relpath(p, src).replace(os.sep, "/")] = open(p, "rb").read()
    return out


def cases():
    return sorted(c for c in os.listdir(CASES)
                  if os.path.isdir(os.path.join(CASES, c, "src")) and os.path.isfile(os.path.join(CASES, c, c + ".tar.gz")))


def check(case):
    """The differences between the archive of a case and its sources, as sentences."""
    src = sources(case)
    problems = []
    packed = {}
    with tarfile.open(os.path.join(CASES, case, case + ".tar.gz")) as t:
        for m in t.getmembers():
            name = m.name[2:] if m.name.startswith("./") else m.name
            if m.isfile() and "/" in name:
                packed[name.split("/", 1)[1]] = t.extractfile(m).read()
    for extra in ("README.md", "MANIFEST.txt"):
        if extra not in src:
            packed.pop(extra, None)
    for p in sorted(set(src) | set(packed)):
        if p not in packed:
            problems.append(f"{p} is in src/ and not in the archive")
        elif p not in src:
            problems.append(f"{p} is in the archive and not in src/")
        elif src[p] != packed[p]:
            problems.append(f"{p} differs between src/ and the archive")
    manifest = os.path.join(CASES, case, "MANIFEST.txt")
    if os.path.isfile(manifest):
        listed = {}
        for line in open(manifest, encoding="utf-8"):
            if line.startswith("#") or not line.strip():
                continue
            parts = line.split()
            listed[parts[-1]] = parts[0]
        for p, b in src.items():
            if p in listed and listed[p] != hashlib.sha256(b).hexdigest():
                problems.append(f"MANIFEST.txt gives another sha256 for {p}")
    if len(problems) > 6:
        problems = problems[:5] + [f"… and {len(problems) - 5} more"]
    return [f"cases/{case}: {x} — run `python3 tools/pack_case.py {case}`" for x in problems]


def pack(case):
    base = os.path.join(CASES, case)
    src = sources(case)
    sha = {p: hashlib.sha256(b).hexdigest() for p, b in src.items()}
    lines = {p: b.count(b"\n") for p, b in src.items()}
    index = os.path.join(base, "index.json")
    if os.path.isfile(index):
        d = json.load(open(index, encoding="utf-8"))
        known = {f["path"] for f in d["file_list"]}
        if known != set(src):
            sys.exit(f"cases/{case}: index.json lists other files than src/ "
                     f"(only in src: {sorted(set(src) - known)[:5]}; only in the index: {sorted(known - set(src))[:5]}) "
                     "— give each new file its `kind` in index.json first")
        kinds = {k: 0 for k in d["lines_by_kind"]}
        for f in d["file_list"]:
            f["sha256"], f["lines"] = sha[f["path"]], lines[f["path"]]
            kinds[f["kind"]] += f["lines"]
        d["lines_by_kind"], d["files"] = kinds, len(d["file_list"])
        with open(index, "w", encoding="utf-8") as out:
            json.dump(d, out, indent=1, ensure_ascii=False)
            out.write("\n")
        order = [f["path"] for f in d["file_list"]]
    else:
        order = sorted(src)
    manifest = os.path.join(base, "MANIFEST.txt")
    old = open(manifest, encoding="utf-8").read().splitlines() if os.path.isfile(manifest) else []
    head = [l for l in old if l.startswith("#")]
    with_lines = any(len(l.split()) == 3 for l in old if l and not l.startswith("#"))
    with open(manifest, "w", encoding="utf-8") as out:
        for l in head:
            out.write(l + "\n")
        for p in order:
            out.write(f"{sha[p]}  {lines[p]:>6}  {p}\n" if with_lines else f"{sha[p]}  {p}\n")
    with tarfile.open(os.path.join(base, case + ".tar.gz"), "w:gz") as t:
        def add(name, data):
            info = tarfile.TarInfo(f"{case}/{name}")
            info.size, info.mode = len(data), 0o644
            t.addfile(info, io.BytesIO(data))
        for p in sorted(src):
            add(p, src[p])
        for extra in ("README.md", "MANIFEST.txt"):
            if extra not in src and os.path.isfile(os.path.join(base, extra)):
                add(extra, open(os.path.join(base, extra), "rb").read())
    print(f"cases/{case}: {len(src)} files packed, {sum(lines.values())} lines")


if __name__ == "__main__":
    args = sys.argv[1:]
    if args == ["--check"]:
        found = [p for c in cases() for p in check(c)]
        for p in found:
            print(p)
        sys.exit(1 if found else 0)
    if not args or args[0].startswith("-"):
        sys.exit(__doc__)
    for c in args:
        pack(c)
