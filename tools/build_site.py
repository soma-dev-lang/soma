#!/usr/bin/env python3
"""Build the machine-readable half of soma-lang.dev.

Everything an agent fetches is GENERATED here from the repo and from the
`soma` binary, so the site cannot drift from the language:

    site/llms-full.txt        the whole language in one fetch
    site/docs/*.md            reference, builtins, gotchas, spec (raw markdown)
    site/builtins.json        `soma describe --builtins --json`
    site/gotchas.json         AGENT_GOTCHAS.md, one entry per wrong->right pair
    site/corpus/index.json    light catalog of the verified corpus (+ the files)
    site/corpus/full.json     the same with summaries; corpus/<domain>/index.json
    site/corpus/<domain>/*.cell
    site/skill/SKILL.md       drop-in agent skill
    site/version.json         what this build was generated from
    site/sitemap.xml

Hand-written (not touched): index.html, agents.html, paper.html, llms.txt,
404.html, agent.md, robots.txt, _headers, _redirects, install.sh, setup.sh, repo/.

    python3 tools/build_site.py                 # uses `soma` from PATH
    SOMA=compiler/target/release/soma python3 tools/build_site.py
    python3 tools/build_site.py --no-verify     # skip re-running check/test

Deploy (Cloudflare Pages, direct upload):
    wrangler pages deploy site --project-name soma-lang
"""
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SITE = os.path.join(ROOT, "site")
CORPUS = os.path.join(ROOT, "examples", "corpus")
BASE = "https://soma-lang.dev"
SOMA = os.environ.get("SOMA", "soma")
if os.sep in SOMA:
    SOMA = os.path.abspath(SOMA)  # commands run from each example's directory
VERIFY = "--no-verify" not in sys.argv


def read(*parts):
    with open(os.path.join(ROOT, *parts), encoding="utf-8") as f:
        return f.read()


def write(rel, text):
    path = os.path.join(SITE, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    return rel


def soma(*args, cwd=None):
    env = dict(os.environ)
    env.pop("SOMA_LLM_MOCK", None)  # the corpus must pass as a stranger runs it
    return subprocess.run(
        [SOMA, *args], capture_output=True, text=True, cwd=cwd, env=env, timeout=120
    )


def soma_version():
    out = soma("--version").stdout.strip()
    return out.split()[-1] if out else "unknown"


# ── corpus ───────────────────────────────────────────────────────────

FEATURES = [
    ("state_machine", re.compile(r"^\s*state\s+\w+", re.M)),
    ("invariant", re.compile(r"^\s*invariant\s", re.M)),
    ("agent", re.compile(r"^cell\s+agent\s", re.M)),
    ("sum_type", re.compile(r"^cell\s+type\s", re.M)),
    ("tests", re.compile(r"^cell\s+test\s", re.M)),
    ("http", re.compile(r"\bon\s+request\s*\(")),
    ("native", re.compile(r"\[native")),
    ("think", re.compile(r"\bthink(_json)?\s*\(")),
    ("pipeline", re.compile(r"\|>")),
    ("match", re.compile(r"\bmatch\s")),
    ("try", re.compile(r"\btry\s*\{")),
    ("lambda", re.compile(r"=>")),
    ("guard", re.compile(r"\bguard\s*\{")),
    ("fail", re.compile(r"\bfail\s*\(|\brequire\b")),
    ("cost", re.compile(r"^\s*cost\s*\{", re.M)),
    ("tools", re.compile(r"^\s*tool\s+\w+\(", re.M)),
]


def structural_features(src):
    """Features that need a count rather than a pattern."""
    out = []
    cells = re.findall(r"^cell\s+(?!test\b|type\b)(?:agent\s+)?(\w+)", src, re.M)
    if len(cells) >= 2:
        out.append("multi_cell")
    if re.search(r"\bon\s+request\s*\(", src) and re.search(r"^\s*invariant\s", src, re.M) and re.search(r"^\s*state\s+\w+", src, re.M):
        out.append("service")  # http + invariant + lifecycle: a whole verified service
    return out


def header_comment(src):
    """The leading // block: first sentence = title, the rest = summary."""
    lines = []
    for line in src.splitlines():
        s = line.strip()
        if s.startswith("//"):
            lines.append(s[2:].strip())
        elif s == "" and lines:
            break
        elif s:
            break
    # drop usage lines ("soma run ...") and banner rules (==== / ----)
    prose = [
        l for l in lines
        if l and not l.startswith("soma ") and not re.fullmatch(r"[=\-─━_*#~\s]{4,}", l)
    ]
    text = re.sub(r"\s+", " ", " ".join(prose)).strip()
    # title: the first line of prose, completed to the end of its sentence
    # when the line break fell mid-sentence
    title = prose[0] if prose else ""
    if title and title[-1] not in ".!?:" and len(prose) > 1:
        m = re.match(r"(.+?[.!?])(\s|$)", text)
        if m and len(m.group(1)) <= 160:
            title = m.group(1)
    title = title.rstrip(" .")
    usage = [l for l in lines if l.startswith("soma ")]
    return title, text, usage


def verify_one(path):
    rel = os.path.relpath(path, ROOT)
    d = os.path.dirname(path)
    res = {}
    for cmd in ("check", "test", "verify"):
        r = soma(cmd, path, cwd=d)
        out = r.stdout + r.stderr
        if cmd == "verify":
            m = re.search(r"(\d+ passed, \d+ warnings?, \d+ failures?)", re.sub(r"\x1b\[[0-9;]*m", "", out))
            t = re.search(r"Temporal: (\d+ passed, \d+ failed)", out)
            if m:
                res["verify_summary"] = m.group(1) + (f"; temporal {t.group(1)}" if t else "")
        if cmd == "test" and "no test cells found" in out:
            res[cmd] = None
        elif cmd == "verify" and "No state machines found" in out:
            res[cmd] = None
        else:
            res[cmd] = r.returncode == 0
    return rel, res


def build_corpus(version):
    files = []
    for domain in sorted(os.listdir(CORPUS)):
        ddir = os.path.join(CORPUS, domain)
        if not os.path.isdir(ddir):
            continue
        for name in sorted(os.listdir(ddir)):
            if name.endswith(".cell"):
                files.append((domain, name, os.path.join(ddir, name)))

    verdicts = {}
    if VERIFY:
        with ThreadPoolExecutor(8) as ex:
            for rel, res in ex.map(verify_one, [p for _, _, p in files]):
                verdicts[rel] = res

    out_dir = os.path.join(SITE, "corpus")
    if os.path.isdir(out_dir):
        shutil.rmtree(out_dir)
    entries, failing = [], []
    for domain, name, path in files:
        src = open(path, encoding="utf-8").read()
        title, summary, usage = header_comment(src)
        rel = os.path.relpath(path, ROOT)
        v = verdicts.get(rel)
        if v is not None and not all(x in (True, None) for k, x in v.items() if k != "verify_summary"):
            failing.append((rel, v))
            continue  # never publish an example the toolchain rejects
        dst = os.path.join(out_dir, domain, name)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(path, dst)
        entry = {
            "id": f"{domain}/{name[:-5]}",
            "domain": domain,
            "title": title,
            "summary": summary,
            "url": f"{BASE}/corpus/{domain}/{name}",
            "repo_path": rel,
            "lines": src.count("\n") + 1,
            "features": [k for k, rx in FEATURES if rx.search(src)] + structural_features(src),
            "cells": re.findall(r"^cell\s+(?:agent\s+|type\s+|test\s+)?(\w+)", src, re.M),
        }
        if usage:
            entry["usage"] = usage
        if v is not None:
            entry["verified"] = {k: x for k, x in v.items() if x is not None and k != "verify_summary"}
            if v.get("verify_summary"):
                entry["verify"] = v["verify_summary"]
        # the soma.toml a program is verified with (its [verify] properties,
        # its [agent] mock) is part of the program
        manifest = os.path.join(os.path.dirname(path), "soma.toml")
        if os.path.exists(manifest):
            shutil.copyfile(manifest, os.path.join(out_dir, domain, "soma.toml"))
            entry["manifest"] = f"{BASE}/corpus/{domain}/soma.toml"
        entries.append(entry)

    # a feature nearly every program has carries no information
    for feat in {f for e in entries for f in e["features"]}:
        if sum(1 for e in entries if feat in e["features"]) > 0.9 * len(entries):
            for e in entries:
                e["features"] = [f for f in e["features"] if f != feat]

    domains = {}
    for e in entries:
        domains[e["domain"]] = domains.get(e["domain"], 0) + 1
    index = {
        "description": "Verified Soma programs. Every entry passed `soma check` "
        "(and `soma test` / `soma verify` where applicable) on the soma "
        "version below, re-run when this index was generated. Fetch `url` "
        "for the source. Filter by `domain` or `features` to find a working "
        "example of what you are about to write.",
        "soma_version": version,
        "count": len(entries),
        "domains": domains,
        "features": sorted({f for e in entries for f in e["features"]}),
        "programs": entries,
    }
    # full.json: everything (the `soma example` CLI searches summaries).
    write("corpus/full.json", json.dumps(index, ensure_ascii=False))
    # index.json: light enough for a model's context — no summaries.
    light = dict(index)
    light["description"] += (
        " This is the light index; /corpus/<domain>/index.json and "
        "/corpus/full.json add a `summary` per program."
    )
    light["programs"] = [
        {k: e[k] for k in ("id", "title", "features", "lines", "url")} for e in entries
    ]
    write("corpus/index.json", json.dumps(light, indent=0, ensure_ascii=False))
    # domains.json: ~1 KB — where to look before fetching anything bigger
    write("corpus/domains.json", json.dumps({
        "description": "Domains of the verified corpus. Fetch /corpus/<domain>/index.json (≈12 KB, with "
                       "summaries) for one domain, /corpus/index.json (≈80 KB) to filter all programs by feature.",
        "soma_version": version,
        "count": len(entries),
        "features": {f: sum(1 for e in entries if f in e["features"]) for f in index["features"]},
        "domains": [
            {"domain": d, "programs": n, "index": f"{BASE}/corpus/{d}/index.json"}
            for d, n in sorted(domains.items())
        ],
    }, indent=1, ensure_ascii=False))
    for domain in domains:
        sub = dict(index)
        sub["count"] = domains[domain]
        sub["domains"] = {domain: domains[domain]}
        sub["programs"] = [e for e in entries if e["domain"] == domain]
        write(f"corpus/{domain}/index.json", json.dumps(sub, indent=1, ensure_ascii=False))

    # one plain-text listing too: cheap to read, greppable
    lines = [f"# Soma verified corpus — {len(entries)} programs (soma {version})", ""]
    cur = None
    for e in entries:
        if e["domain"] != cur:
            cur = e["domain"]
            lines += ["", f"## {cur}", ""]
        feats = ", ".join(e["features"])
        lines.append(f"- [{e['id']}]({e['url']}): {e['title']} [{feats}]")
    write("corpus/index.md", "\n".join(lines) + "\n")
    return entries, failing


# ── gotchas ──────────────────────────────────────────────────────────

def build_gotchas():
    md = read("AGENT_GOTCHAS.md")
    items = []
    for m in re.finditer(r"^## (\d+)\. (.+?)\n(.*?)(?=^## |\Z)", md, re.M | re.S):
        n, title, body = int(m.group(1)), m.group(2).strip(), m.group(3).strip()
        blocks = re.findall(r"```soma\n(.*?)```", body, re.S)
        errs = re.findall(r"//\s*(error|warning):\s*(.+)", body)
        item = {"n": n, "title": title, "markdown": body}
        if blocks:
            item["wrong"] = blocks[0].strip()
        if len(blocks) > 1:
            item["right"] = blocks[1].strip()
        if errs:
            item["diagnostic"] = f"{errs[0][0]}: {errs[0][1].strip()}"
        items.append(item)
    write(
        "gotchas.json",
        json.dumps(
            {
                "description": "Mistakes models make writing Soma, each with the real "
                "compiler diagnostic and the fix. Verified against the soma binary.",
                "gotchas": items,
            },
            indent=1,
            ensure_ascii=False,
        ),
    )
    return items


# ── page examples ────────────────────────────────────────────────────

def verify_page_examples():
    """Every complete program shown on an HTML page (a <pre><code> block
    starting with `cell `) must pass check, and test if it has tests. A
    page that teaches broken Soma fails the build."""
    import html as htmllib
    import tempfile

    broken = []
    for page in ("index.html", "agents.html"):
        src = read("site", page)
        blocks = re.findall(r"<pre><code>(cell .*?)</code></pre>", src, re.S)
        for i, block in enumerate(blocks):
            code = htmllib.unescape(re.sub(r"<[^>]+>", "", block)) + "\n"
            with tempfile.TemporaryDirectory() as d:
                f = os.path.join(d, "page.cell")
                open(f, "w", encoding="utf-8").write(code)
                for cmd in ("check", "test"):
                    if cmd == "test" and "cell test " not in code:
                        continue
                    r = soma(cmd, f, cwd=d)
                    if r.returncode != 0:
                        first = (r.stdout + r.stderr).strip().splitlines()
                        broken.append(f"{page} example #{i + 1}: soma {cmd} failed — {first[0] if first else ''}")
    return broken


# ── main ─────────────────────────────────────────────────────────────

def main():
    version = soma_version()
    today = datetime.date.today().isoformat()
    print(f"soma {version} ({SOMA})")

    # Keep the historical repository entry point on the canonical short guide.
    with open(os.path.join(ROOT, "AGENT.md"), "w", encoding="utf-8") as f:
        f.write("<!-- Generated from site/agent.md by tools/build_site.py. -->\n\n" + read("site", "agent.md"))

    # docs, raw markdown
    docs = {
        "docs/reference.md": "SOMA_REFERENCE.md",
        "docs/builtins.md": "SOMA_BUILTINS.md",
        "docs/gotchas.md": "AGENT_GOTCHAS.md",
        "docs/guarantees.md": "docs/site/guarantees.md",
        "docs/serving.md": "docs/site/serving.md",
        "docs/operations.md": "docs/site/operations.md",
        "CHANGELOG.md": "CHANGELOG.md",
        "LICENSE": "LICENSE",
    }
    # SOMA_BUILTINS.md is generated: refresh it from the binary first (the
    # committed copy said "181 builtins" while the binary had 207)
    r = soma("docs", "builtins")
    with open(os.path.join(ROOT, "SOMA_BUILTINS.md"), "w") as f:
        f.write(r.stdout)
    for dst, src in docs.items():
        write(dst, read(src))
    stale = os.path.join(SITE, "docs", "spec.md")  # SOMA_SPEC.md says "Version: 2.2.1"
    if os.path.exists(stale):
        os.remove(stale)
    write("skill/SKILL.md", read("SKILL.md"))

    # builtins, straight from the compiler
    r = soma("describe", "--builtins", "--json")
    builtins = json.loads(r.stdout)
    # "reserved" names are tracked for replay but NOT callable: an agent
    # that read `timestamp() -> Int` here wrote it and got "not a builtin".
    # `deterministic: false` means the call is recorded by --record and
    # replayed by `soma replay` — say so in every entry, not in a README.
    builtins = [b for b in builtins if b.get("category") != "reserved"]
    write("builtins.json", json.dumps(builtins, indent=1, ensure_ascii=False))

    gotchas = build_gotchas()
    entries, failing = build_corpus(version)

    # llms-full.txt: one fetch, the whole language
    llms = read("site", "llms.txt")
    full = [
        llms.rstrip(),
        "\n\n---\n\n# PART 2 — Language reference (SOMA_REFERENCE.md)\n\n" + read("SOMA_REFERENCE.md").strip(),
        "\n\n---\n\n# PART 3 — Verified wrong→right pairs (AGENT_GOTCHAS.md)\n\n" + read("AGENT_GOTCHAS.md").strip(),
        "\n\n---\n\n# PART 4 — Every builtin (SOMA_BUILTINS.md, generated from the compiler)\n\n" + read("SOMA_BUILTINS.md").strip(),
        "\n",
    ]
    full_text = "".join(full)
    write("llms-full.txt", full_text)

    # llms-service.txt: what you need to build a verified service, without
    # the linear-algebra / quant reference (~half the size of llms-full)
    service = [
        llms.rstrip(),
        "\n\n---\n\n" + read("docs/site/guarantees.md").strip(),
        "\n\n---\n\n" + read("docs/site/serving.md").strip(),
        "\n\n---\n\n" + read("docs/site/operations.md").strip(),
        "\n\n---\n\n# Verified wrong→right pairs\n\n" + read("AGENT_GOTCHAS.md").strip(),
        "\n",
    ]
    write("llms-service.txt", "".join(service))

    import hashlib
    # the hash an agent can reproduce with `soma docs agent | shasum -a 256`
    # on the released binary — so it is computed from the BINARY's copy
    embedded = soma("docs", "agent").stdout
    docs_sha = hashlib.sha256(embedded.encode("utf-8")).hexdigest()
    if embedded != llms:
        print("WARNING: site/llms.txt differs from the binary's embedded copy — rebuild the compiler first")

    write(
        "version.json",
        json.dumps(
            {
                "soma_version": version,
                "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, text=True).strip()),
                "agent_md_sha256": hashlib.sha256(read("site", "agent.md").encode("utf-8")).hexdigest(),
                "generated": today,
                "license": "MIT",
                # `soma docs agent | shasum -a 256` on the matching binary
                # prints this: the site and the binary say the same thing
                "llms_txt_sha256": docs_sha,
                "corpus_programs": len(entries),
                "builtins": len(builtins),
                "gotchas": len(gotchas),
                "resources": {
                    "llms": f"{BASE}/llms.txt",
                    "llms_full": f"{BASE}/llms-full.txt",
                    "llms_service": f"{BASE}/llms-service.txt",
                    "guarantees": f"{BASE}/docs/guarantees.md",
                    "serving": f"{BASE}/docs/serving.md",
                    "operations": f"{BASE}/docs/operations.md",
                    "corpus_domains": f"{BASE}/corpus/domains.json",
                    "changelog": f"{BASE}/CHANGELOG.md",
                    "license": f"{BASE}/LICENSE",
                    "status": f"{BASE}/status",
                    "agent_block": f"{BASE}/agent.md",
                    "skill": f"{BASE}/skill/SKILL.md",
                    "builtins": f"{BASE}/builtins.json",
                    "gotchas": f"{BASE}/gotchas.json",
                    "corpus_index": f"{BASE}/corpus/index.json",
                    "registry": f"{BASE}/repo/index.json",
                },
            },
            indent=1,
        ),
    )

    # sitemap: every page and every machine file (not the 316 sources —
    # corpus/index.json is their sitemap)
    pages = [
        "", "paper", "agents", "status", "llms.txt", "llms-service.txt", "llms-full.txt", "agent.md",
        "skill/SKILL.md", "docs/guarantees.md", "docs/serving.md", "docs/reference.md", "docs/builtins.md",
        "docs/gotchas.md", "builtins.json", "gotchas.json", "corpus/domains.json", "corpus/index.json",
        "corpus/index.md", "CHANGELOG.md", "LICENSE",
        "repo/index.json", "version.json",
    ]
    sm = ['<?xml version="1.0" encoding="UTF-8"?>',
          '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
    for p in pages:
        sm.append(f"  <url><loc>{BASE}/{p}</loc><lastmod>{today}</lastmod></url>")
    sm.append("</urlset>")
    write("sitemap.xml", "\n".join(sm) + "\n")

    broken = verify_page_examples() if VERIFY else []
    for b in broken:
        print(f"  BROKEN {b}")
    print(f"corpus: {len(entries)} published, {len(failing)} withheld")
    for rel, v in failing:
        print(f"  WITHHELD {rel}: {v}")
    print(f"builtins: {len(builtins)}  gotchas: {len(gotchas)}")
    size = sum(
        os.path.getsize(os.path.join(d, f))
        for d, _, fs in os.walk(SITE)
        for f in fs
    )
    print(f"site/: {size // 1024} KiB")
    if failing or broken:
        sys.exit(1)


if __name__ == "__main__":
    main()
