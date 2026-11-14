"""The HTML half of soma-lang.dev: one page per thing worth knowing.

The site's first readers are language models — at inference time through
llms.txt and the JSON indexes, and at TRAINING time through the crawls
that become pre-training corpora. Those crawls keep the text of HTML pages
and drop most .txt, .json and .cell files. So everything an agent needs to
write Soma is published here a second time, as plain static HTML:

    /                          what Soma is, with a proof run on the page
    /learn/<lesson>            the course, one lesson per page
    /guide/<topic>             the agent guide and the long-form docs
    /reference/<section>       SOMA_REFERENCE.md, one section per page
    /builtins/<category>       every builtin, with the programs that use it
    /errors/<CODE>             every diagnostic: a wrong program, the real
                               diagnostic, the fixed program
    /gotchas/<n>               the verified wrong -> right pairs
    /programs/<domain>/<name>  every corpus program with what check, test,
                               verify and run printed on it
    /cli/<command>             every command's --help
    /changelog/<version>       the release notes

Rules every page follows, because a page may be read on its own, out of
context, years from now:
  - it names the language ("Soma, a programming language") and the version
    the page was checked with;
  - every complete program on it passed the toolchain at build time, and
    every output on it was printed by the binary, not written by hand;
  - it is static HTML, no script, readable as text; code is in <pre><code>;
  - it links to its neighbours, so a crawler reaches all of it from `/`.
"""
import html
import json
import os
import re
import shutil
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor

import sitemd
from sitemd import BASE, inline, render, slug, text_of

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SITE = os.path.join(ROOT, "site")
REPO = "https://github.com/soma-dev-lang/soma"
SECTIONS = ["learn", "guide", "reference", "builtins", "errors", "gotchas", "programs", "cli", "changelog"]

NAV = [
    ("/learn/", "Learn"),
    ("/guide/", "Guide"),
    ("/reference/", "Reference"),
    ("/builtins/", "Builtins"),
    ("/errors/", "Errors"),
    ("/programs/", "Programs"),
    ("/agents", "For agents"),
]


LAST = {}


class Site:
    def __init__(self, soma, version, date, commit):
        self.soma = soma
        self.version = version
        self.date = date
        self.commit = commit
        self.pages = []      # site-relative URLs, for the sitemap
        self.problems = []
        self.builtin_url = {}
        self.cmds = set()
        self.verdicts = {}
        self.judged = {"pass": 0, "fail": 0}

    # ── linking ──────────────────────────────────────────────────────

    def link(self, code):
        """URL for a code span, when it names something with a page."""
        m = re.fullmatch(r"([EW]\d{4})", code)
        if m and os.path.isdir(os.path.join(ROOT, "docs", "site", "errors", code)):
            return f"/errors/{code}"
        m = re.fullmatch(r"soma (\w+)\b.*", code)
        if m and m.group(1) in self.cmds:
            return f"/cli/{m.group(1)}"
        m = re.fullmatch(r"(\w+)(\(.*\))?", code)
        if m and m.group(1) in self.builtin_url and (m.group(2) or len(m.group(1)) > 3):
            return self.builtin_url[m.group(1)]
        return None

    def md(self, text, **kw):
        return render(text, links=self.link, judge=self.judge, **kw)

    def judge(self, lang, code):
        """For a block that is a whole program: one sentence with what
        `soma check` says of it, so that no page shows code that does not
        compile without saying so. Fragments get nothing."""
        if lang not in ("soma", "") or "..." in code or "…" in code:
            return None
        body = "\n".join(l for l in code.splitlines() if not l.strip().startswith("//")).strip()
        if not re.match(r"(cell|skeleton)\s", body) or body.count("{") != body.count("}") or not body.endswith("}"):
            return None
        if re.search(r"^\s*(use|flesh)\s", body, re.M):
            return None  # needs its other files
        if code not in self.verdicts:
            with tempfile.TemporaryDirectory() as d:
                with open(os.path.join(d, "program.cell"), "w", encoding="utf-8") as f:
                    f.write(code + "\n")
                rc, out = check_in(self.soma, d, "program.cell", json_out=True)
            try:
                j = json.loads(out)
            except ValueError:
                j = {"passed": False, "errors": [{"message": out.splitlines()[0] if out else "no output"}]}
            self.verdicts[code] = j
        j = self.verdicts[code]
        v = self.version
        if not j.get("errors"):
            self.judged["pass"] += 1
            return (f'<p class="check ok">A complete Soma program: <code>soma check</code> passes on it '
                    f'(soma {html.escape(v)}).</p>')
        self.judged["fail"] += 1
        e = j["errors"][0]
        code_ = f' ({e["code"]})' if e.get("code") else ""
        msg = re.sub(r"\s+", " ", e.get("message", ""))[:300]
        return (f'<p class="check bad">As shown, this does not compile: <code>soma check</code> reports '
                f'<q>{html.escape(msg)}</q>{html.escape(code_)}.</p>')

    # ── the page ─────────────────────────────────────────────────────

    def write(self, path, title, body, description, crumbs=(), toc=None, side=None,
              prev=None, nxt=None, ld_type="TechArticle", alt=None, extra_ld=None,
              h1=True, full_title=None):
        """path: "errors/E0028" or "errors/" (an index) or "" (home)."""
        url = "/" + path
        file = os.path.join(SITE, path + "index.html" if path.endswith("/") or path == "" else path + ".html")
        os.makedirs(os.path.dirname(file), exist_ok=True)
        self.pages.append(url)
        page_title = full_title or f"{title} — Soma programming language"
        crumb_html = ""
        if crumbs:
            items = [f'<a href="/">Soma</a>'] + [f'<a href="{u}">{html.escape(t)}</a>' for u, t in crumbs]
            crumb_html = '<nav class="crumbs" aria-label="Breadcrumb">' + " / ".join(items) + "</nav>"
        side_html = ""
        if toc and len(toc) > 2:
            side_html += '<nav class="toc" aria-label="On this page"><h2>On this page</h2><ul>' + "".join(
                f'<li><a href="#{a}">{inline(t)}</a></li>' for a, t in toc) + "</ul></nav>"
        if side:
            side_html += side
        pn = ""
        if prev or nxt:
            pn = '<nav class="pn">'
            pn += f'<a class="prev" href="{prev[0]}">← {html.escape(prev[1])}</a>' if prev else "<span></span>"
            pn += f'<a class="next" href="{nxt[0]}">{html.escape(nxt[1])} →</a>' if nxt else "<span></span>"
            pn += "</nav>"
        ld = {
            "@context": "https://schema.org",
            "@type": ld_type,
            ("headline" if ld_type == "TechArticle" else "name"): title,
            "description": description,
            "url": BASE + url,
            "inLanguage": "en",
            "dateModified": self.date,
            "license": "https://opensource.org/licenses/MIT",
            "isPartOf": {"@type": "WebSite", "name": "Soma programming language", "url": BASE + "/"},
            "about": {"@type": "ComputerLanguage", "name": "Soma", "url": BASE + "/"},
        }
        if extra_ld:
            ld.update(extra_ld)
        alt_html = ""
        for href, typ, t in (alt or []):
            alt_html += f'<link rel="alternate" type="{typ}" href="{href}" title="{html.escape(t)}">\n'
        cur = ' aria-current="page"'
        nav = "".join(
            f'<a href="{u}"{cur if url.startswith(u.rstrip("/")) else ""}>{t}</a>' for u, t in NAV)
        heading = f"<h1>{inline(title)}</h1>" if h1 else ""
        page = TEMPLATE.format(
            title=html.escape(page_title),
            description=html.escape(description, quote=True),
            canonical=BASE + url,
            alt=alt_html,
            ld=json.dumps(ld, ensure_ascii=False).replace("</", "<\\/"),
            nav=nav,
            crumbs=crumb_html,
            heading=heading,
            body=body,
            pn=pn,
            side=f'<aside>{side_html}</aside>' if side_html else "",
            layout="with-side" if side_html else "single",
            version=html.escape(self.version),
            date=self.date,
            commit=self.commit[:7],
            commit_full=self.commit,
            css=CSS,
        )
        with open(file, "w", encoding="utf-8") as f:
            f.write(inline_kbd(page))

    def section_nav(self, title, items, current=None):
        """A sidebar list of a section's pages: every page reaches its siblings."""
        li = []
        for u, t in items:
            cur = ' aria-current="page"' if u == current else ""
            li.append(f'<li><a href="{u}"{cur}>{inline(t)}</a></li>')
        return f'<nav class="sect"><h2>{html.escape(title)}</h2><ul>{"".join(li)}</ul></nav>'


TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<link rel="canonical" href="{canonical}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:url" content="{canonical}">
<meta property="og:type" content="article">
<meta property="og:site_name" content="Soma programming language">
<meta name="generator" content="tools/build_pages.py, soma {version}">
<link rel="alternate" type="text/plain" href="/llms.txt" title="The Soma language guide for models">
<link rel="alternate" type="text/plain" href="/llms-full.txt" title="The whole Soma language in one file">
{alt}<script type="application/ld+json">{ld}</script>
<style>{css}</style>
</head>
<body>
<header class="top">
<a class="brand" href="/">soma<span>.cell</span></a>
<nav aria-label="Sections">{nav}</nav>
</header>
<div class="page {layout}">
<main>
{crumbs}
<article>
{heading}
{body}
</article>
{pn}
</main>
{side}
</div>
<footer class="site">
<p><strong>Soma</strong> is an open-source programming language (MIT) for programs written by coding agents: one <code>.cell</code> file holds the data, the state machine, the handlers, the tests and the HTTP routes, and <code>soma verify</code> proves the lifecycle and the limits it can. This page was generated from the <a href="{repo}">Soma repository</a> at commit <a href="{repo}/commit/{commit_full}">{commit}</a>; every program and every command output on it was produced by <strong>soma {version}</strong> on {date}.</p>
<p>Everything on soma-lang.dev may be read, indexed, quoted and used to train models. For models at inference time: <a href="/llms.txt">llms.txt</a> · <a href="/llms-full.txt">llms-full.txt</a> · <a href="/builtins.json">builtins.json</a> · <a href="/corpus/index.json">corpus/index.json</a> · <a href="/version.json">version.json</a>.</p>
</footer>
</body>
</html>
""".replace("{repo}", REPO)

CSS = """
:root{color-scheme:light dark;--bg:#fcfcfa;--fg:#1c1d1f;--muted:#55595f;--faint:#80858c;--line:#e3e4e0;--panel:#f3f4f0;--code-bg:#f5f6f2;--accent:#0b6e5f;--accent-2:#0d8a77;--ok:#1f7a3a;--bad:#a8322c;--warn:#8a5a00;--mono:ui-monospace,SFMono-Regular,"JetBrains Mono",Menlo,Consolas,monospace}
@media (prefers-color-scheme:dark){:root{--bg:#121414;--fg:#e6e7e3;--muted:#a6aba7;--faint:#7c817d;--line:#2b2f2e;--panel:#191c1c;--code-bg:#171a1a;--accent:#5cc8b2;--accent-2:#7dd9c5;--ok:#79d18f;--bad:#ff8f87;--warn:#e6b450}}
*{box-sizing:border-box}
html{-webkit-text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--fg);font:16.5px/1.65 -apple-system,BlinkMacSystemFont,"Segoe UI",Inter,Roboto,sans-serif}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
code,pre,kbd{font-family:var(--mono);font-size:.88em}
kbd{font-family:var(--mono);font-size:.88em;background:var(--panel);border:1px solid var(--line);border-radius:4px;padding:.05em .3em;white-space:break-spaces}
a>kbd{color:var(--accent)}
header.top{display:flex;align-items:center;gap:1.4rem;padding:.75rem max(16px,calc((100vw - 1180px)/2));border-bottom:1px solid var(--line);flex-wrap:wrap;position:sticky;top:0;background:var(--bg);z-index:5}
.brand{font:700 1.15rem var(--mono);color:var(--fg);letter-spacing:-.02em}.brand span{color:var(--accent)}
header.top nav{display:flex;gap:1.1rem;flex-wrap:wrap;font-size:.93rem}
header.top nav a{color:var(--muted)}header.top nav a[aria-current]{color:var(--fg);font-weight:600}
.page{max-width:1180px;margin:0 auto;padding:1.6rem 16px 3rem;display:grid;gap:3rem}
.page.with-side{grid-template-columns:minmax(0,1fr) 250px}
.page.single main{max-width:860px}
main{min-width:0}
article{min-width:0}
h1{font-size:2.05rem;line-height:1.15;letter-spacing:-.02em;margin:.4rem 0 1rem}
h2{font-size:1.38rem;line-height:1.25;margin:2.4rem 0 .7rem;letter-spacing:-.01em}
h3{font-size:1.1rem;margin:1.8rem 0 .5rem}
h4{font-size:1rem;margin:1.4rem 0 .4rem}
p,ul,ol{margin:.7rem 0}li{margin:.25rem 0}
.lede{font-size:1.15rem;color:var(--muted)}
.crumbs{font-size:.86rem;color:var(--faint)}.crumbs a{color:var(--faint)}
div.code{margin:1.1rem 0}
p.file{display:inline-block;margin:0;font:600 .78rem var(--mono);color:var(--muted);background:var(--panel);border:1px solid var(--line);border-bottom:0;border-radius:6px 6px 0 0;padding:.25rem .7rem}
pre{margin:0;padding:.9rem 1rem;background:var(--code-bg);border:1px solid var(--line);border-radius:0 8px 8px 8px;overflow-x:auto;line-height:1.5;tab-size:4}
div.code>pre:first-child{border-radius:8px}
div.out pre{border-left:3px solid var(--ok)}
div.out.bad pre{border-left-color:var(--bad)}
p.check{font-size:.9rem;margin:1rem 0 -.6rem;color:var(--muted)}
p.check.ok::before{content:"✓ ";color:var(--ok);font-weight:700}
p.check.bad::before{content:"✗ ";color:var(--bad);font-weight:700}
blockquote{margin:1rem 0;padding:.2rem 1rem;border-left:3px solid var(--line);color:var(--muted)}
.table{overflow-x:auto;margin:1rem 0}
table{border-collapse:collapse;width:100%;font-size:.93rem}
th,td{text-align:left;vertical-align:top;padding:.45rem .6rem;border-bottom:1px solid var(--line)}
th{font-weight:600;background:var(--panel)}
hr{border:0;border-top:1px solid var(--line);margin:2rem 0}
aside{font-size:.9rem;position:sticky;top:4.2rem;align-self:start;max-height:calc(100vh - 5rem);overflow:auto}
aside h2{font-size:.78rem;text-transform:uppercase;letter-spacing:.06em;color:var(--faint);margin:0 0 .5rem}
aside ul{list-style:none;padding:0;margin:0 0 1.6rem}aside li{margin:.3rem 0;line-height:1.35}
aside a{color:var(--muted)}aside a[aria-current]{color:var(--fg);font-weight:600}
.pn{display:flex;justify-content:space-between;gap:1rem;margin-top:3rem;padding-top:1rem;border-top:1px solid var(--line);font-size:.95rem}
.meta{font-size:.9rem;color:var(--muted)}
.tags{display:flex;flex-wrap:wrap;gap:.35rem;margin:.6rem 0;padding:0;list-style:none}
.tags li{margin:0}.tags a,.tags span{display:inline-block;font:.78rem var(--mono);padding:.1rem .5rem;border:1px solid var(--line);border-radius:99px;color:var(--muted);background:var(--panel)}
.verdict{font:600 .8rem var(--mono);padding:.1rem .45rem;border-radius:4px;border:1px solid currentColor}
.verdict.ok{color:var(--ok)}.verdict.bad{color:var(--bad)}.verdict.warn{color:var(--warn)}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:1rem;margin:1.2rem 0}
.card{border:1px solid var(--line);border-radius:10px;padding:1rem 1.1rem;background:var(--panel)}
.card h3{margin:0 0 .35rem;font-size:1.02rem}.card p{margin:.3rem 0;font-size:.93rem;color:var(--muted)}
.install{font:.95rem var(--mono);background:var(--code-bg);border:1px solid var(--line);border-radius:8px;padding:.7rem 1rem;overflow-x:auto;white-space:nowrap}
dl.sig dt{font:600 .95rem var(--mono);margin-top:1.6rem}dl.sig dd{margin:.3rem 0 0}
footer.site{border-top:1px solid var(--line);color:var(--muted);font-size:.86rem;padding:1.4rem max(16px,calc((100vw - 1180px)/2)) 2.5rem}
footer.site p{max-width:860px}
@media (max-width:900px){.page.with-side{grid-template-columns:1fr}aside{position:static;max-height:none;order:2}h1{font-size:1.7rem}}
"""


def inline_kbd(page):
    """Inline code as <kbd>, block code as <pre><code>. Text extractors
    (trafilatura, the FineWeb default) treat <code> as a block and break the
    sentence around every inline one; <kbd> stays in the sentence, and comes
    out between backticks when they keep formatting."""
    parts = re.split(r"(<pre\b.*?</pre>|<script\b.*?</script>)", page, flags=re.S)
    for i in range(0, len(parts), 2):
        parts[i] = re.sub(r"<code(\s[^>]*)?>", r"<kbd\1>", parts[i]).replace("</code>", "</kbd>")
    return "".join(parts)


# ── helpers ──────────────────────────────────────────────────────────

def read(*parts):
    with open(os.path.join(ROOT, *parts), encoding="utf-8") as f:
        return f.read()


def split_h2(md):
    """[(title or None for the preamble, markdown)] split on `## `, fences respected."""
    parts, cur, title, fence = [], [], None, False
    for line in md.splitlines():
        if line.lstrip().startswith("```"):
            fence = not fence
        if not fence and line.startswith("## "):
            parts.append((title, "\n".join(cur)))
            title, cur = line[3:].strip(), []
            continue
        cur.append(line)
    parts.append((title, "\n".join(cur)))
    return parts


def strip_h1(md):
    """(h1 text, the markdown without it)."""
    m = re.search(r"^# (.+)$", md, re.M)
    if not m:
        return None, md
    return m.group(1).strip(), md[:m.start()] + md[m.end():]


def plain(s):
    return re.sub(r"[`*]", "", re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", s)).strip()


def console(cmd, out, bad=False):
    text = f"$ {cmd}" + ("\n" + out if out else "")
    cls = "code out bad" if bad else "code out"
    return f'<div class="{cls}"><pre><code class="language-console">{html.escape(text)}</code></pre></div>'


def source(name, code, lang="soma"):
    return (f'<div class="code src"><p class="file">{html.escape(name)}</p>'
            f'<pre><code class="language-{lang}">{html.escape(code.rstrip())}</code></pre></div>')


# ── learn ────────────────────────────────────────────────────────────

def build_learn(site):
    md, problems = sitemd.execute(read("docs", "site", "learn.md"), site.soma, where="learn")
    site.problems += problems
    parts = split_h2(md)
    title, intro = strip_h1(parts[0][1])
    lessons = []
    for t, body in parts[1:]:
        m = re.match(r"Lesson (\d+)\s*[—-]\s*(.+)", t)
        s = f"{m.group(1)}-{slug(m.group(2))}" if m else slug(t)
        lessons.append((s, t, body))
    items = [(f"/learn/{s}", plain(t)) for s, t, _ in lessons]
    side = site.section_nav("Lessons", items)
    body, _ = site.md(intro)
    body += '<h2 id="lessons">The lessons</h2><ol>'
    for s, t, b in lessons:
        body += f'<li><a href="/learn/{s}">{inline(t)}</a> — {html.escape(text_of(b, 180))}</li>'
    body += "</ol>"
    body += ('<p class="meta">The same course as one plain-text file for models: '
             '<a href="/llms-learn.txt">/llms-learn.txt</a>. Source: '
             f'<a href="{REPO}/blob/main/docs/site/learn.md">docs/site/learn.md</a>.</p>')
    site.write("learn/", title, body, text_of(intro), crumbs=[("/learn/", "Learn")], side=side,
               alt=[("/llms-learn.txt", "text/plain", "This course as plain text")],
               nxt=items[0] if items else None)
    for k, (s, t, b) in enumerate(lessons):
        html_body, toc = site.md(b, shift=0)
        html_body = (f'<p class="meta">A lesson of the course on the Soma programming language. Every output on this page '
                     f'was printed by soma {site.version}: the build writes each file to an empty folder and runs each command.</p>' + html_body)
        site.write(f"learn/{s}", plain(t), html_body, text_of(b),
                   crumbs=[("/learn/", "Learn")], side=site.section_nav("Lessons", items, f"/learn/{s}"),
                   prev=items[k - 1] if k else ("/learn/", "Course overview"),
                   nxt=items[k + 1] if k + 1 < len(items) else ("/guide/", "The guide"))


# ── guide (long-form documents) ──────────────────────────────────────

GUIDES = [
    ("language", "site/llms.txt", "The language guide",
     "The guide `soma docs agent` prints: the whole core language in one page, as an agent reads it before writing Soma."),
    ("skeleton", "site/llms-skeleton.txt", "Large programs, skeleton first",
     "Write the structure first: memory, machines and each signal's frame; then fill bodies one brief at a time."),
    ("flesh", "site/llms-flesh.txt", "Writing a flesh body",
     "What a flesh body may do, and how `soma check` holds it to its frame."),
    ("agents", "site/llms-agents.txt", "Agents, think() and hordes",
     "LLM calls inside a state machine: think(), tools, budgets, [task] handlers and hordes."),
    ("native", "site/llms-native.txt", "Native handlers",
     "`[native]` handlers compiled to Rust for numeric hot paths, and what they may contain."),
    ("guarantees", "docs/site/guarantees.md", "What is proven, checked, or not covered",
     "The exact guarantees of `soma verify` and of the runtime, and their limits."),
    ("serving", "docs/site/serving.md", "Serving HTTP",
     "What `soma serve` exposes, how routes map to handlers and errors to statuses."),
    ("operations", "docs/site/operations.md", "Operations",
     "Running a Soma service: storage, restarts, failures, and what happens when it goes wrong."),
    ("cluster", "docs/site/cluster.md", "Cluster replication",
     "Eventual replication between nodes: setup, guarantees and limits."),
    ("robotics", "docs/robotics/ROBUSTNESS.md", "Robustness for robotics",
     "Bounded loops, timing and interlocks for control programs."),
    ("agent-loop", "site/agent.md", "Working on a Soma project, as an agent",
     "The block a Soma project puts in its AGENTS.md: the loop to run, what each command proves, and the limits to state."),
    ("releases", "site/llms-releases.txt", "Recent releases, for agents",
     "What changed in recent releases, written for an agent that learned an older Soma."),
]


def build_guides(site):
    items = [(f"/guide/{s}", t) for s, _, t, _ in GUIDES]
    cards = ""
    for k, (s, src, t, d) in enumerate(GUIDES):
        md = read(src)
        md = re.sub(r"^<!--.*?-->\s*", "", md, flags=re.S)
        h1, rest = strip_h1(md)
        body, toc = site.md(rest)
        intro = f'<p class="lede">{inline(d, site.link)} Part of the guide to the Soma programming language.</p>'
        alt = []
        if src.startswith("site/"):
            name = os.path.basename(src)
            alt = [(f"/{name}", "text/plain", "This page as plain text for models")]
            intro += f'<p class="meta">Also as plain text: <a href="/{name}">/{name}</a>.</p>'
        site.write(f"guide/{s}", t, intro + body, plain(d), crumbs=[("/guide/", "Guide")], toc=toc,
                   side=site.section_nav("Guide", items, f"/guide/{s}"), alt=alt,
                   prev=items[k - 1] if k else None, nxt=items[k + 1] if k + 1 < len(items) else None)
        cards += f'<div class="card"><h3><a href="/guide/{s}">{html.escape(t)}</a></h3><p>{inline(d, site.link)}</p></div>'
    body = ('<p class="lede">The long-form documents of Soma. Start with <a href="/guide/language">the language guide</a> '
            '— it is what <code>soma docs agent</code> prints, and it fits in one context window.</p>'
            f'<div class="grid">{cards}</div>')
    site.write("guide/", "Guide", body, "The Soma programming language guide: the core language, large programs written skeleton first, agents, native code, guarantees, serving and operations.",
               crumbs=[("/guide/", "Guide")])


# ── reference ────────────────────────────────────────────────────────

def build_reference(site):
    parts = split_h2(read("SOMA_REFERENCE.md"))
    h1, pre = strip_h1(parts[0][1])
    secs = [(slug(t), t, b) for t, b in parts[1:]]
    items = [(f"/reference/{s}", t) for s, t, _ in secs]
    for k, (s, t, b) in enumerate(secs):
        body, toc = site.md(b, shift=0)
        # sub-sections are ### in the source: list them as the page's toc
        toc = [(slug(x), x) for x in re.findall(r"^### (.+)$", b, re.M)]
        body, _ = site.md(re.sub(r"^###", "##", b, flags=re.M))
        body = (f'<p class="lede">{html.escape(plain(t))}, in the reference of the Soma programming language '
                f'(soma {html.escape(site.version)}). A block marked ✓ is a whole program that <code>soma check</code> '
                'accepts; the others are excerpts.</p>') + body
        desc = text_of(b) or f"Soma language reference: {plain(t)}."
        site.write(f"reference/{s}", f"{plain(t)}", body, desc, crumbs=[("/reference/", "Reference")],
                   toc=toc, side=site.section_nav("Reference", items, f"/reference/{s}"),
                   prev=items[k - 1] if k else ("/reference/", "Reference"),
                   nxt=items[k + 1] if k + 1 < len(items) else ("/builtins/", "Builtins"),
                   full_title=f"{plain(t)} — Soma language reference")
    body, _ = site.md(pre)
    body += "<ul>" + "".join(f'<li><a href="{u}">{inline(t)}</a></li>' for u, t in items) + "</ul>"
    body += ('<p class="meta">The whole reference as one file: <a href="/docs/reference.md">/docs/reference.md</a>. '
             'Every builtin with its exact signature: <a href="/builtins/">Builtins</a>.</p>')
    site.write("reference/", "Language reference", body,
               "The Soma language reference, one section per page: cell structure, types, control flow, collections, pipes, storage, state machines, sum types, HTTP, tests, skeleton and flesh.",
               crumbs=[("/reference/", "Reference")], alt=[("/docs/reference.md", "text/markdown", "The reference as markdown")])


# ── builtins ─────────────────────────────────────────────────────────

CATEGORY = {
    "string": "Strings", "math": "Math", "collection": "Collections", "pipeline": "Pipelines",
    "lambda": "Lambdas", "types": "Types and conversion", "io": "Files and I/O", "time": "Time",
    "http": "HTTP client", "web": "Web responses", "template": "Templates", "agent": "Agents and LLM calls",
    "state": "State machines", "memory": "Memory", "native": "Native code", "linalg": "Linear algebra and risk",
    "internal": "Internal",
}


def builtin_uses(entries, builtins):
    """name -> [(program entry, line number, line)] — a few real uses each."""
    names = {b["name"] for b in builtins}
    uses = {n: [] for n in names}
    for e in entries:
        own = set(re.findall(r"\bon\s+(\w+)\s*\(", e["_src"]))
        for no, line in enumerate(e["_src"].splitlines(), 1):
            if line.strip().startswith("//"):
                continue
            for n in set(re.findall(r"(?<![\w.])(\w+)\s*\(", line)) & names - own:
                if len(uses[n]) < 4 and all(u[0]["id"] != e["id"] for u in uses[n]):
                    uses[n].append((e, no, line.strip()))
    return uses


def build_builtins(site, builtins, entries):
    cats = {}
    for b in builtins:
        cats.setdefault(b["category"], []).append(b)
    order = [c for c in CATEGORY if c in cats] + sorted(c for c in cats if c not in CATEGORY)
    for c in order:
        for b in cats[c]:
            site.builtin_url[b["name"]] = f"/builtins/{c}#{b['name']}"
    uses = builtin_uses(entries, builtins)
    items = [(f"/builtins/{c}", f"{CATEGORY.get(c, c)} ({len(cats[c])})") for c in order]
    for k, c in enumerate(order):
        body = f'<p class="lede">{len(cats[c])} builtins of Soma {html.escape(site.version)} in the category <strong>{html.escape(CATEGORY.get(c, c))}</strong>, with their exact signatures, as <code>soma describe --builtins --json</code> prints them.</p>'
        body += "<ul>" + "".join(f'<li><a href="#{b["name"]}"><code>{html.escape(b["name"])}</code></a></li>' for b in cats[c]) + "</ul>"
        for b in cats[c]:
            body += f'<h2 id="{b["name"]}"><code>{html.escape(b["name"])}</code></h2>'
            body += f'<pre><code class="language-soma">{html.escape(b["signature"])}</code></pre>'
            body += f'<p>{inline(b["brief"], site.link)}</p>'
            det = "deterministic" if b.get("deterministic") else "not deterministic"
            body += f'<p class="meta">{det} · replay: {html.escape(str(b.get("replay", "")))}</p>'
            if uses.get(b["name"]):
                body += "<p>Used in verified programs:</p><ul>"
                for e, no, line in uses[b["name"]]:
                    body += (f'<li><a href="/programs/{e["id"]}">{html.escape(e["id"])}</a>, line {no}: '
                             f'<code>{html.escape(line[:140])}</code></li>')
                body += "</ul>"
        title = f"{CATEGORY.get(c, c)} builtins"
        desc = f"Soma builtins for {CATEGORY.get(c, c).lower()}: " + ", ".join(b["name"] for b in cats[c][:12]) + ("…" if len(cats[c]) > 12 else "") + " — exact signatures and real uses."
        site.write(f"builtins/{c}", title, body, desc, crumbs=[("/builtins/", "Builtins")],
                   side=site.section_nav("Categories", items, f"/builtins/{c}"),
                   prev=items[k - 1] if k else None, nxt=items[k + 1] if k + 1 < len(items) else None,
                   alt=[("/builtins.json", "application/json", "Every builtin as JSON")])
    body = (f'<p class="lede">Soma {html.escape(site.version)} has {len(builtins)} builtins. This list is generated from the compiler '
            '(<code>soma describe --builtins --json</code>), so a name that is not here is not a builtin — an agent should '
            'never guess one.</p><div class="table"><table><thead><tr><th>Builtin</th><th>Signature</th></tr></thead><tbody>')
    for c in order:
        body += f'<tr><th colspan="2"><a href="/builtins/{c}">{html.escape(CATEGORY.get(c, c))}</a></th></tr>'
        for b in cats[c]:
            body += (f'<tr><td><a href="/builtins/{c}#{b["name"]}"><code>{html.escape(b["name"])}</code></a></td>'
                     f'<td><code>{html.escape(b["signature"])}</code></td></tr>')
    body += "</tbody></table></div>"
    site.write("builtins/", "Builtins", body,
               f"Every builtin of the Soma programming language ({len(builtins)}), with exact signatures generated from the compiler.",
               crumbs=[("/builtins/", "Builtins")], side=site.section_nav("Categories", items),
               alt=[("/builtins.json", "application/json", "Every builtin as JSON")])


# ── errors ───────────────────────────────────────────────────────────

def explain_list(soma):
    out = subprocess.run([soma, "explain"], capture_output=True, text=True).stdout
    codes = []
    for line in out.splitlines():
        m = re.match(r"^([EW]\d{4})\s+(\S+)\s+(.*)$", line)
        if m:
            codes.append(m.groups())
    return codes


def check_in(soma, d, name, json_out=False):
    args = [soma, "check"] + (["--json"] if json_out else []) + [name]
    r = subprocess.run(args, cwd=d, capture_output=True, text=True, env=sitemd.clean_env(), timeout=60)
    out = sitemd.ANSI.sub("", r.stdout + r.stderr)
    for p in (os.path.realpath(d) + os.sep, d + os.sep):  # no build folder on a page
        out = out.replace(p, "")
    return r.returncode, out.rstrip()


def error_page_data(soma, code):
    src = os.path.join(ROOT, "docs", "site", "errors", code)
    if not os.path.isdir(src):
        return None
    files = sorted(f for f in os.listdir(src) if os.path.isfile(os.path.join(src, f)) and not f.startswith('.'))
    data = {"note": open(os.path.join(src, "note.md"), encoding="utf-8").read().strip() if "note.md" in files else ""}
    if "unreachable" in files:
        data["unreachable"] = open(os.path.join(src, "unreachable"), encoding="utf-8").read().strip()
        return data
    import check_error_pairs
    for side in ("wrong", "right", "example"):
        f = side + ".cell"
        if f not in files:
            continue
        with tempfile.TemporaryDirectory() as d:
            check_error_pairs.stage(src, d, side)
            data[f] = open(os.path.join(src, f), encoding="utf-8", errors="replace").read()
            data[f + ".out"] = check_in(soma, d, f)
            rc, j = check_in(soma, d, f, json_out=True)
            try:
                data[f + ".json"] = json.loads(j)
            except ValueError:
                pass
            if os.path.exists(os.path.join(d, "soma.toml")):
                data[side + ".toml"] = open(os.path.join(d, "soma.toml"), encoding="utf-8").read()
    return data


def build_errors(site, gotchas):
    sys_path_tools = os.path.dirname(os.path.abspath(__file__))
    import sys
    sys.path.insert(0, sys_path_tools)
    import check_error_pairs
    codes = explain_list(site.soma)
    with ThreadPoolExecutor(8) as ex:
        datas = list(ex.map(lambda c: error_page_data(site.soma, c[0]), codes))
        pair_problems = list(ex.map(lambda c: check_error_pairs.check_pair(c[0], site.soma), codes))
    for p in pair_problems:
        site.problems += [f"errors: {x}" for x in p if "no directory" not in x]
    items = [(f"/errors/{c}", f"{c} {n}") for c, n, _ in codes]
    by_code = {}
    for g in gotchas:
        for c in set(re.findall(r"\b([EW]\d{4})\b", g.get("markdown", ""))):
            by_code.setdefault(c, []).append(g)
    for k, ((code, name, desc), data) in enumerate(zip(codes, datas)):
        kind = "warning" if code.startswith("W") else "error"
        body = f'<p class="lede"><code>{code}</code> <code>{html.escape(name)}</code> — {inline(desc, site.link)}.</p>'
        body += (f'<p>A {"warning" if kind == "warning" else "compile-time error"} of <code>soma check</code>, '
                 f'the checker of the Soma programming language (<code>soma explain {code}</code>).</p>')
        if data and data.get("note"):
            body += "<h2 id=\"why\">The rule</h2>" + site.md(data["note"])[0]
        if data and "wrong.cell" in data and "right.cell" in data:
            j = data.get("wrong.cell.json") or {}
            diag = next((x for x in j.get("errors", []) + j.get("warnings", []) if x.get("code") == code), None)
            body += '<h2 id="wrong">A program that triggers it</h2>'
            if "wrong.toml" in data:
                body += source("soma.toml", data["wrong.toml"], "toml")
            body += source("wrong.cell", data["wrong.cell"])
            rc, out = data["wrong.cell.out"]
            body += "<p>What <code>soma check</code> prints:</p>" + console("soma check wrong.cell", out, bad=rc != 0)
            if diag and diag.get("fix"):
                body += f'<p>The <code>fix</code> field of <code>soma check --json</code>: <q>{html.escape(diag["fix"])}</q></p>'
            body += '<h2 id="right">The fix</h2>'
            if data.get("right.toml") and data.get("right.toml") != data.get("wrong.toml"):
                body += source("soma.toml", data["right.toml"], "toml")
            body += fix_html(data["wrong.cell"], data["right.cell"])
            rc, out = data["right.cell.out"]
            body += "<p>With the fix, <kbd>soma check</kbd> accepts the program:</p>"
            body += console("soma check right.cell", out, bad=rc != 0)
        elif data and "example.cell" in data:
            body += '<h2 id="example">A program on which it is reported</h2>'
            body += source("example.cell", data["example.cell"])
            rc, out = data["example.cell.out"]
            body += console("soma check example.cell", out)
        elif data and data.get("unreachable"):
            body += f'<h2 id="when">When you see it</h2>{site.md(data["unreachable"])[0]}'
        if code in by_code:
            body += '<h2 id="gotchas">Related pitfalls</h2><ul>' + "".join(
                f'<li><a href="/gotchas/{g["n"]}">{inline(g["title"])}</a></li>' for g in by_code[code]) + "</ul>"
        title = f"{code}: {name}"
        site.write(f"errors/{code}", title, body,
                   f"Soma {kind} {code} ({name}): {desc}. A program that triggers it, the real diagnostic, and the fix.",
                   crumbs=[("/errors/", "Errors")],
                   prev=items[k - 1] if k else None, nxt=items[k + 1] if k + 1 < len(items) else None,
                   full_title=f"{code} {name} — Soma diagnostic")
    body = (f'<p class="lede">Every diagnostic code of <code>soma check</code> ({len(codes)}), each with a program that triggers it, '
            'the diagnostic the compiler really prints, and the fixed program. Errors (E) stop the program; warnings (W) do not.</p>'
            '<p>An agent reads them with <code>soma check --json</code>: each diagnostic carries <code>code</code>, '
            '<code>message</code>, <code>fix</code>, <code>line</code> and <code>col</code>. The message names the form to write.</p>'
            '<div class="table"><table><thead><tr><th>Code</th><th>Name</th><th>Meaning</th></tr></thead><tbody>')
    for code, name, desc in codes:
        body += f'<tr><td><a href="/errors/{code}"><code>{code}</code></a></td><td><code>{html.escape(name)}</code></td><td>{inline(desc)}</td></tr>'
    body += "</tbody></table></div>"
    site.write("errors/", "Diagnostics", body,
               f"Every diagnostic of the Soma compiler ({len(codes)} codes): what it means, a program that triggers it, the real output and the fix.",
               crumbs=[("/errors/", "Errors")])


def fix_html(wrong, right):
    """The fix as sentences and a diff, not as a second copy of the program:
    the lines that change, in the edit format a model has to produce."""
    import difflib
    a, b = wrong.rstrip().splitlines(), right.rstrip().splitlines()
    sm = difflib.SequenceMatcher(None, a, b)
    said, diff = [], []
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op == "equal":
            continue
        old, new = [x.strip() for x in a[i1:i2]], [x.strip() for x in b[j1:j2]]
        where = f"line {i1 + 1}" if i2 - i1 <= 1 else f"lines {i1 + 1}–{i2}"
        if op == "replace" and len(old) == 1 and len(new) == 1:
            said.append(f"on {where}, <kbd>{html.escape(old[0])}</kbd> becomes <kbd>{html.escape(new[0])}</kbd>")
        elif op == "delete":
            said.append(f"{where} {'is' if len(old) == 1 else 'are'} removed")
        elif op == "insert":
            said.append(f"{len(new)} line{'s are' if len(new) > 1 else ' is'} added after line {i1}")
        else:
            said.append(f"{where} {'is' if i2 - i1 == 1 else 'are'} rewritten")
    for line in difflib.unified_diff(a, b, "wrong.cell", "right.cell", n=2, lineterm=""):
        diff.append(line)
    text = ("; ".join(said) + ".") if said else "The fix is in the manifest."
    out = f"<p>{text[0].upper() + text[1:]}</p>"
    if diff:
        out += (f'<div class="code src"><p class="file">wrong.cell → right.cell</p>'
                f'<pre><code class="language-diff">{html.escape(chr(10).join(diff))}</code></pre></div>')
    return out


# ── gotchas ──────────────────────────────────────────────────────────

def build_gotchas(site, gotchas):
    items = [(f"/gotchas/{g['n']}", f"{g['n']}. {plain(g['title'])}") for g in gotchas]
    for k, g in enumerate(gotchas):
        body, toc = site.md(g["markdown"])
        body = (f'<p class="lede">Pitfall {g["n"]} of writing Soma, the programming language: a habit from another language, '
                'or a rule that surprises, with what the Soma compiler answers and the form to write.</p>') + body
        site.write(f"gotchas/{g['n']}", plain(g["title"]), body,
                   f"Soma pitfall {g['n']}: {plain(g['title'])}. The mistake, the compiler's answer and the right form.",
                   crumbs=[("/gotchas/", "Pitfalls")], side=site.section_nav("Pitfalls", items, f"/gotchas/{g['n']}"),
                   prev=items[k - 1] if k else None, nxt=items[k + 1] if k + 1 < len(items) else None,
                   alt=[("/gotchas.json", "application/json", "Every pitfall as JSON")])
    md = read("AGENT_GOTCHAS.md")
    pre = md.split("\n## ", 1)[0]
    h1, pre = strip_h1(pre)
    body, _ = site.md(pre)
    body += "<ol>" + "".join(f'<li value="{g["n"]}"><a href="/gotchas/{g["n"]}">{inline(g["title"])}</a></li>' for g in gotchas) + "</ol>"
    site.write("gotchas/", "Pitfalls: mistakes models make writing Soma", body,
               "The mistakes language models make when they write Soma, each with the compiler's real answer and the right form.",
               crumbs=[("/gotchas/", "Pitfalls")], alt=[("/gotchas.json", "application/json", "Every pitfall as JSON")])


# ── programs ─────────────────────────────────────────────────────────

FEATURE_TEXT = {
    "state_machine": "a state machine", "invariant": "a memory invariant", "agent": "an agent cell",
    "sum_type": "a sum type", "tests": "test cells", "http": "HTTP routes", "native": "[native] handlers",
    "think": "LLM calls (think)", "pipeline": "pipes (|>)", "match": "match", "try": "try / catch",
    "lambda": "lambdas", "guard": "guard blocks", "fail": "require / fail", "cost": "a cost bound",
    "tools": "tools", "skeleton": "a skeleton", "multi_cell": "several cells", "service": "a whole verified service",
}


def nice_title(t):
    """"CHARGEBACK WINDOW — disputes die…" -> "Chargeback window — disputes die…"."""
    head, sep, rest = t.partition(" — ")
    if sep and head.upper() == head and any(c.isalpha() for c in head):
        head = head[:1] + head[1:].lower()
    elif not sep and t.upper() == t:
        return t[:1] + t[1:].lower()
    return head + sep + rest


def program_outputs(soma, domain_dir, name, usage):
    """Run check / test / verify / the usage lines on a COPY of the program's
    folder (a run must not see state left by a previous build)."""
    outs = []
    with tempfile.TemporaryDirectory() as d:
        for f in os.listdir(domain_dir):
            p = os.path.join(domain_dir, f)
            if os.path.isfile(p) and (f == name or not f.endswith(".cell")):
                shutil.copyfile(p, os.path.join(d, f))
        for cmd in ("check", "test", "verify"):
            code, out = sitemd.run_cmd(f"soma {cmd} {name}", d, soma, timeout=120)
            if cmd == "test" and "no test cells found" in out:
                continue
            if cmd == "verify" and "No state machines found" in out:
                continue
            outs.append((f"soma {cmd} {name}", code, out))
        for u in usage:
            u = re.sub(r"\S*/" + re.escape(name), name, u.strip())
            if not u.startswith("soma run ") or "serve" in u:
                continue
            code, out = sitemd.run_cmd(u, d, soma, timeout=30)
            outs.append((re.sub(r"\s+", " ", u), code, out))
        r = subprocess.run([soma, "describe", name], cwd=d, capture_output=True, text=True, env=sitemd.clean_env())
        try:
            desc = json.loads(r.stdout)
        except ValueError:
            desc = {}
    return outs, desc


def words(xs, conj="and"):
    xs = list(xs)
    if len(xs) <= 1:
        return "".join(xs)
    return ", ".join(xs[:-1]) + f" {conj} " + xs[-1]


def structure_prose(desc, src):
    """What `soma describe` says about a program, as sentences: the shape
    of the code in words, next to the code."""
    cells = desc.get("cells") or []
    if not cells:
        return ""
    c = lambda x: f"<code>{html.escape(str(x))}</code>"
    paras = []
    for cell in cells:
        sent = [f"The cell {c(cell['name'])}"]
        sigs = (cell.get("face") or {}).get("signals") or []
        parts = []
        if sigs:
            parts.append(f"declares {len(sigs)} signal{'s' if len(sigs) > 1 else ''} in its face: " + words(
                c(f"{g['name']}({', '.join(g.get('params', []))})" + (f" -> {g['returns']}" if g.get("returns") else "")) for g in sigs))
        mem = cell.get("memory") or []
        if mem:
            ms = []
            for m in mem:
                props = m.get("properties") or []
                t = f"{c(m['name'])} ({c(m.get('type', ''))}" + (f", {words(props)}" if props else "") + ")"
                if m.get("invariants"):
                    t += " under the invariant " + words((c(i) for i in m["invariants"]), "and")
                ms.append(t)
            parts.append(f"keeps {len(mem)} memory slot{'s' if len(mem) > 1 else ''}: " + words(ms))
        sent.append(("; ".join(parts) if parts else "has no face and no memory") + ".")
        para = " ".join(sent)
        for sm in cell.get("state_machines") or []:
            tr = sm.get("transitions") or []
            outgoing = {t["from"] for t in tr}
            terminal = [x for x in sm.get("states", []) if x not in outgoing and x != "*"]
            edges = words(c(f"{t['from']} -> {t['to']}") + (" (guarded)" if t.get("has_guard") else "") for t in tr)
            para += (f" Its state machine {c(sm['name'])} starts in {c(sm.get('initial'))} and allows "
                     f"{len(tr)} transition{'s' if len(tr) != 1 else ''}: {edges}.")
            if terminal:
                para += f" {words(c(x) for x in terminal)} {'is' if len(terminal) == 1 else 'are'} terminal."
        hs = [h for h in cell.get("handlers") or [] if not h["name"].startswith("_")]
        priv = [h for h in cell.get("handlers") or [] if h["name"].startswith("_")]
        if hs:
            para += " Handlers: " + words(f"{c(h['name'])} (line {h['line']})" for h in hs) + "."
        if priv:
            para += f" Private helpers: {words(c(h['name']) for h in priv)}."
        paras.append(para)
    tests = re.findall(r"^cell\s+test\s+(\w+)", src, re.M)
    if tests:
        n = len(re.findall(r"^\s*(assert|assert_fails)\b", src, re.M))
        paras.append(f"The file also holds {len(tests)} test cell{'s' if len(tests) > 1 else ''} "
                     f"({words(c(t) for t in tests)}) with {n} assertion{'s' if n != 1 else ''}, run by <code>soma test</code>.")
    return '<h2 id="structure">How it is built</h2>' + "".join(f"<p>{p}</p>" for p in paras) + \
        '<p class="meta">Generated from <code>soma describe</code> on this file.</p>'


def build_programs(site, entries):
    corpus = os.path.join(ROOT, "examples", "corpus")
    jobs = [(e, os.path.join(corpus, e["domain"]), e["id"].split("/", 1)[1] + ".cell") for e in entries]
    with ThreadPoolExecutor(8) as ex:
        results = list(ex.map(lambda j: program_outputs(site.soma, j[1], j[2], j[0].get("usage", [])), jobs))
    outs = [r[0] for r in results]
    descs = [r[1] for r in results]
    domains = {}
    for e in entries:
        if e["summary"].startswith(e["title"]):  # the summary opens with the title
            e["summary"] = e["summary"][len(e["title"]):].lstrip(" .:—-")
        e["title"] = nice_title(e["title"])
        domains.setdefault(e["domain"], []).append(e)
    feats = {}
    for e in entries:
        for f in e["features"]:
            feats.setdefault(f, []).append(e)
    for (e, ddir, name), out, desc in zip(jobs, outs, descs):
        sibs = domains[e["domain"]]
        k = sibs.index(e)
        title = e["title"] or name
        summary = e["summary"]
        cells = ", ".join(f"<code>{html.escape(c)}</code>" for c in e["cells"])
        body = f'<p class="lede">{html.escape(summary)}</p>' if summary and summary != title else ""
        body += (f'<p>A Soma program from the <a href="/programs/{e["domain"]}/">{html.escape(e["domain"])}</a> part of the verified corpus: '
                 f'{e["lines"]} lines, cells {cells}. It passes <code>soma check</code>'
                 + (", <code>soma test</code>" if e.get("verified", {}).get("test") else "")
                 + (" and <code>soma verify</code>" if e.get("verified", {}).get("verify") else "")
                 + f' with soma {html.escape(site.version)}.</p>')
        if e["features"]:
            body += '<ul class="tags">' + "".join(
                f'<li><a href="/programs/feature/{f}">{html.escape(FEATURE_TEXT.get(f, f))}</a></li>' for f in e["features"]) + "</ul>"
        src = e["_src"]
        body += structure_prose(desc, src)
        body += '<h2 id="source">The source</h2>'
        if e.get("manifest"):
            body += source("soma.toml", open(os.path.join(ddir, "soma.toml"), encoding="utf-8").read(), "toml")
        body += source(name, src)
        if out:
            body += '<h2 id="toolchain">What the toolchain says</h2>'
            body += '<p class="meta">Run on a fresh copy of the program at build time; the outputs are pasted unedited.</p>'
            for cmd, code, text in out:
                if len(text) > 6000:
                    text = text[:6000].rsplit("\n", 1)[0] + "\n… (truncated)"
                body += console(cmd, text, bad=code != 0)
        used = sorted({n for n in re.findall(r"(?<![\w.])(\w+)\s*\(", re.sub(r"//.*", "", src)) if n in site.builtin_url}
                      - set(re.findall(r"\bon\s+(\w+)\s*\(", src)))
        if used:
            body += '<h2 id="builtins">Builtins used</h2><p>' + ", ".join(
                f'<a href="{site.builtin_url[n]}"><code>{n}</code></a>' for n in used) + "</p>"
        body += f'<p class="meta">Source file: <a href="/corpus/{e["id"]}.cell">/corpus/{e["id"]}.cell</a> · <a href="{REPO}/blob/main/{e["repo_path"]}">{html.escape(e["repo_path"])}</a></p>'
        site.write(f"programs/{e['id']}", title, body,
                   (summary or title)[:300],
                   crumbs=[("/programs/", "Programs"), (f"/programs/{e['domain']}/", e["domain"])],
                   prev=(f"/programs/{sibs[k - 1]['id']}", sibs[k - 1]["title"] or sibs[k - 1]["id"]) if k else None,
                   nxt=(f"/programs/{sibs[k + 1]['id']}", sibs[k + 1]["title"] or sibs[k + 1]["id"]) if k + 1 < len(sibs) else None,
                   ld_type="SoftwareSourceCode",
                   extra_ld={"programmingLanguage": {"@type": "ComputerLanguage", "name": "Soma"},
                             "codeRepository": REPO, "codeSampleType": "full solution",
                             "runtimePlatform": f"soma {site.version}"},
                   alt=[(f"/corpus/{e['id']}.cell", "text/plain", "The source")])

    def listing(es):
        rows = "".join(
            f'<tr><td><a href="/programs/{e["id"]}">{html.escape(e["title"] or e["id"])}</a></td>'
            f'<td>{e["lines"]}</td><td>{", ".join(html.escape(FEATURE_TEXT.get(f, f)) for f in e["features"])}</td></tr>'
            for e in es)
        return f'<div class="table"><table><thead><tr><th>Program</th><th>Lines</th><th>Uses</th></tr></thead><tbody>{rows}</tbody></table></div>'

    ditems = [(f"/programs/{d}/", f"{d} ({len(es)})") for d, es in sorted(domains.items())]
    fitems = [(f"/programs/feature/{f}", f"{FEATURE_TEXT.get(f, f)} ({len(es)})") for f, es in sorted(feats.items())]
    side = site.section_nav("Domains", ditems) + site.section_nav("By feature", fitems)
    for d, es in sorted(domains.items()):
        readme = ""
        body = (f'<p class="lede">{len(es)} verified Soma programs in the <strong>{html.escape(d)}</strong> domain. Each page shows the source '
                'and what <code>soma check</code>, <code>soma test</code>, <code>soma verify</code> and <code>soma run</code> printed on it.</p>')
        body += listing(es) + readme
        site.write(f"programs/{d}/", f"Programs: {d}", body,
                   f"{len(es)} verified Soma programs in the {d} domain: " + "; ".join((e["title"] or e["id"]) for e in es[:6]) + "…",
                   crumbs=[("/programs/", "Programs")], side=site.section_nav("Domains", ditems, f"/programs/{d}/"),
                   alt=[(f"/corpus/{d}/index.json", "application/json", "This domain as JSON")])
    for f, es in sorted(feats.items()):
        body = (f'<p class="lede">{len(es)} verified Soma programs that use {html.escape(FEATURE_TEXT.get(f, f))}. '
                'Start from one of these when you write a program that needs it.</p>') + listing(es)
        site.write(f"programs/feature/{f}", f"Programs using {FEATURE_TEXT.get(f, f)}", body,
                   f"{len(es)} verified Soma programs that use {FEATURE_TEXT.get(f, f)}.",
                   crumbs=[("/programs/", "Programs")], side=site.section_nav("By feature", fitems, f"/programs/feature/{f}"))
    readme = os.path.join(corpus, "README.md")
    intro = site.md(strip_h1(read("examples", "corpus", "README.md"))[1])[0] if os.path.exists(readme) else ""
    body = (f'<p class="lede">{len(entries)} Soma programs, each re-checked by soma {html.escape(site.version)} when this page was built. '
            'A program the toolchain rejects is withheld, never published.</p>'
            '<p>New to Soma? Start with the <a href="/programs/basics/">basic examples</a>: 97 small programs, commented line by line.</p>')
    body += '<div class="grid">' + "".join(
        f'<div class="card"><h3><a href="/programs/{d}/">{html.escape(d)}</a></h3><p>{len(es)} programs: '
        + html.escape("; ".join((e["title"] or e["id"]) for e in es[:3])) + "…</p></div>"
        for d, es in sorted(domains.items())) + "</div>"
    body += intro
    body += ('<p class="meta">From the command line: <code>soma example invariant state_machine</code> finds programs by feature. '
             'As JSON: <a href="/corpus/index.json">/corpus/index.json</a>.</p>')
    site.write("programs/", "Verified programs", body,
               f"{len(entries)} verified programs written in the Soma programming language, by domain and by feature, with the real output of check, test, verify and run.",
               crumbs=[("/programs/", "Programs")], side=side,
               alt=[("/corpus/index.json", "application/json", "The corpus as JSON")])


# ── basics (site/examples: the small commented programs) ─────────────

def build_basics(site):
    """The 97 small, heavily commented programs of site/examples, one page
    each, with what their `// Run:` lines print."""
    src_dir = os.path.join(SITE, "examples")
    files = sorted(f for f in os.listdir(src_dir) if f.endswith(".cell"))

    def one(f):
        src = open(os.path.join(src_dir, f), encoding="utf-8").read()
        m = re.search(r"^//\s*(\d+)\s*[—-]\s*(.+)$", src, re.M)
        title = m.group(2).strip() if m else f[:-5]
        intro = []
        for line in src.splitlines()[3:]:
            t = line.strip()
            if not t.startswith("//") or re.match(r"//\s*(Run|Expect):", t):
                break
            t = t[2:].strip()
            if re.fullmatch(r"[=\-─]{4,}", t):
                continue
            intro.append(t)
        runs = [r.strip() for r in re.findall(r"^//\s*Run:\s*(soma run .+)$", src, re.M)]
        outs = []
        with tempfile.TemporaryDirectory() as d:
            shutil.copyfile(os.path.join(src_dir, f), os.path.join(d, f))
            code, out = sitemd.run_cmd(f"soma check {f}", d, site.soma)
            outs.append((f"soma check {f}", code, out))
            if "cell test" in src:
                code, out = sitemd.run_cmd(f"soma test {f}", d, site.soma)
                outs.append((f"soma test {f}", code, out))
            for r in runs:
                code, out = sitemd.run_cmd(r, d, site.soma, timeout=30)
                outs.append((r, code, out))
            r = subprocess.run([site.soma, "describe", f], cwd=d, capture_output=True, text=True, env=sitemd.clean_env())
            try:
                desc = json.loads(r.stdout)
            except ValueError:
                desc = {}
        return f, title, " ".join(intro).strip(), src, outs, desc

    with ThreadPoolExecutor(8) as ex:
        pages = list(ex.map(one, files))
    items = [(f"/programs/basics/{f[:-5]}", f"{f[:3]}. {t}") for f, t, *_ in pages]
    for k, (f, title, intro, src, outs, desc) in enumerate(pages):
        n = f[:3]
        body = f'<p class="lede">{html.escape(intro)}</p>' if intro else ""
        body += (f'<p>Example {n} of the basic Soma programs: a small, commented program that teaches one thing. '
                 f'Every output below was printed by soma {html.escape(site.version)} when this page was built.</p>')
        body += structure_prose(desc, src)
        body += '<h2 id="source">The source</h2>' + source(f, src)
        body += '<h2 id="toolchain">What the toolchain says</h2>'
        for cmd, code, text in outs:
            if code != 0 and cmd.startswith("soma check"):
                site.problems.append(f"basics: {f} fails soma check")
            body += console(cmd, text[:6000], bad=code != 0)
        site.write(f"programs/basics/{f[:-5]}", f"{title}", body, (intro or title)[:300],
                   crumbs=[("/programs/", "Programs"), ("/programs/basics/", "basics")],
                   prev=items[k - 1] if k else None, nxt=items[k + 1] if k + 1 < len(items) else None,
                   ld_type="SoftwareSourceCode", full_title=f"{title} — a basic Soma example",
                   extra_ld={"programmingLanguage": {"@type": "ComputerLanguage", "name": "Soma"},
                             "codeSampleType": "full solution"},
                   alt=[(f"/examples/{f}", "text/plain", "The source")])
    rows = "".join(f'<tr><td>{f[:3]}</td><td><a href="/programs/basics/{f[:-5]}">{html.escape(t)}</a></td>'
                   f'<td>{html.escape(i[:140])}</td></tr>' for f, t, i, *_ in pages)
    body = (f'<p class="lede">{len(pages)} small Soma programs, from hello world to a full application, each commented line by '
            'line and run when this page was built. Read them in order to learn the language; the '
            '<a href="/programs/">verified corpus</a> has the larger programs.</p>'
            f'<div class="table"><table><thead><tr><th>#</th><th>Example</th><th>What it shows</th></tr></thead><tbody>{rows}</tbody></table></div>')
    site.write("programs/basics/", "Basic examples", body,
               f"{len(pages)} small commented programs in the Soma programming language, from hello world to a full application, with their real output.",
               crumbs=[("/programs/", "Programs")])
    return len(pages)


# ── cli ──────────────────────────────────────────────────────────────

def build_cli(site):
    top = subprocess.run([site.soma, "--help"], capture_output=True, text=True).stdout
    cmds = []
    for line in top.split("Commands:", 1)[1].split("Options:", 1)[0].splitlines():
        m = re.match(r"^\s{2}(\w+)\s+(.+)$", line)
        if m and m.group(1) != "help":
            cmds.append((m.group(1), m.group(2).strip()))
    site.cmds = {c for c, _ in cmds}
    items = [(f"/cli/{c}", f"soma {c}") for c, _ in cmds]
    for k, (c, d) in enumerate(cmds):
        h = subprocess.run([site.soma, c, "--help"], capture_output=True, text=True).stdout.rstrip()
        body = (f'<p class="lede">{inline(d)}.</p><p><code>soma {c}</code> is a command of <code>soma</code>, the toolchain '
                f'of the Soma programming language (compiler, checker, verifier, test runner and server in one binary). '
                f'Its options, as soma {html.escape(site.version)} prints them:</p>') + console(f"soma {c} --help", h)
        site.write(f"cli/{c}", f"soma {c}", body, f"The soma {c} command of the Soma programming language: {d}",
                   crumbs=[("/cli/", "Command line")], side=site.section_nav("Commands", items, f"/cli/{c}"),
                   prev=items[k - 1] if k else None, nxt=items[k + 1] if k + 1 < len(items) else None)
    body = '<p class="lede">The <code>soma</code> binary is the compiler, the checker, the verifier, the test runner, the HTTP server and the language server.</p>'
    body += console("soma --help", top.rstrip())
    body += '<div class="table"><table><thead><tr><th>Command</th><th>What it does</th></tr></thead><tbody>' + "".join(
        f'<tr><td><a href="/cli/{c}"><code>soma {c}</code></a></td><td>{inline(d)}</td></tr>' for c, d in cmds) + "</tbody></table></div>"
    site.write("cli/", "Command line", body, "Every command of the soma binary: run, serve, check, verify, test, skeleton, holes, explain and more.",
               crumbs=[("/cli/", "Command line")], side=site.section_nav("Commands", items))


# ── changelog ────────────────────────────────────────────────────────

def build_changelog(site):
    parts = split_h2(read("CHANGELOG.md"))
    rels = [(t, b) for t, b in parts[1:]]
    items = []
    for t, _ in rels:
        v = t.split()[0]
        items.append((f"/changelog/{v}", t))
    for k, (t, b) in enumerate(rels):
        v = t.split()[0]
        body = ""
        if k:
            body += (f'<p class="meta">This page describes Soma as of release {html.escape(v)}. Some of it may have changed since: '
                     f'the current release is <a href="/changelog/{rels[0][0].split()[0]}">{html.escape(rels[0][0].split()[0])}</a>, and the current '
                     'language is in the <a href="/guide/language">guide</a>.</p>')
        html_body, toc = site.md(re.sub(r"^###", "##", b, flags=re.M))
        body += html_body
        site.write(f"changelog/{v}", f"Soma {t}", body, f"Release notes of Soma {v}: " + text_of(b, 240),
                   crumbs=[("/changelog/", "Changelog")], toc=toc,
                   prev=items[k + 1] if k + 1 < len(items) else None, nxt=items[k - 1] if k else None)
    body = '<p class="lede">The release notes of every Soma version, newest first.</p><ul>' + "".join(
        f'<li><a href="{u}">{html.escape(t)}</a></li>' for u, t in items) + "</ul>"
    site.write("changelog/", "Changelog", body, "Release notes of the Soma programming language, every version.",
               crumbs=[("/changelog/", "Changelog")], alt=[("/CHANGELOG.md", "text/markdown", "The changelog as markdown")])


# ── hand-written pages (docs/site/pages/*.md, executable) ────────────

def build_written(site):
    pdir = os.path.join(ROOT, "docs", "site", "pages")
    for f in sorted(os.listdir(pdir)):
        if not f.endswith(".md"):
            continue
        name = f[:-3]
        raw = open(os.path.join(pdir, f), encoding="utf-8").read()
        meta = {}
        m = re.match(r"^---\n(.*?)\n---\n", raw, re.S)
        if m:
            for line in m.group(1).splitlines():
                k, _, v = line.partition(":")
                meta[k.strip()] = v.strip()
            raw = raw[m.end():]
        md, problems = sitemd.execute(raw, site.soma, split=None, where=f"pages/{f}")
        site.problems += problems
        title, rest = strip_h1(md)
        body, toc = site.md(rest)
        path = "" if name == "home" else name
        site.write(path, meta.get("title", title), body, meta.get("description", text_of(rest)),
                   toc=toc if meta.get("toc") == "yes" else None, h1=meta.get("h1", "yes") == "yes",
                   full_title=meta.get("full_title"),
                   ld_type=meta.get("type", "TechArticle"))


def build_paper(site):
    src = read("docs", "site", "paper.html")
    m = re.search(r"<body[^>]*>(.*)</body>", src, re.S)
    body = m.group(1) if m else src
    body = re.sub(r"<nav.*?</nav>", "", body, flags=re.S)
    body = re.sub(r"<(script|style)\b.*?</\1>", "", body, flags=re.S)
    t = re.search(r"<h1[^>]*>(.*?)</h1>", body, re.S)
    title = re.sub(r"<[^>]+>", "", t.group(1)).strip() if t else "Soma paper"
    if t:
        body = body.replace(t.group(0), "", 1)
    site.write("paper", title, body, "The Soma paper: cells, verified distribution, signals as replication, evaluation.",
               ld_type="ScholarlyArticle")


def build_404(site):
    body = ('<p class="lede">There is no page at this address.</p><p>The language guide: <a href="/guide/language">/guide/language</a>. '
            'Every diagnostic: <a href="/errors/">/errors/</a>. Every builtin: <a href="/builtins/">/builtins/</a>. '
            'For models: <a href="/llms.txt">/llms.txt</a>.</p>')
    site.write("404", "Not found", body, "Page not found.")
    site.pages.remove("/404")


# ── entry point ──────────────────────────────────────────────────────

def build(soma, version, date, commit, builtins, gotchas, entries):
    """Write every HTML page; return (urls, problems)."""
    for s in SECTIONS:
        d = os.path.join(SITE, s)
        if os.path.isdir(d):
            shutil.rmtree(d)
    for old in ("learn.html",):
        p = os.path.join(SITE, old)
        if os.path.exists(p):
            os.remove(p)
    site = Site(soma, version, date, commit)
    build_cli(site)                       # first: code spans link to commands
    build_builtins(site, builtins, entries)  # then builtins, for the same reason
    build_written(site)
    build_learn(site)
    build_guides(site)
    build_reference(site)
    build_errors(site, gotchas)
    build_gotchas(site, gotchas)
    build_programs(site, entries)
    build_basics(site)
    build_changelog(site)
    build_paper(site)
    build_404(site)
    LAST["judged"] = site.judged
    return site.pages, site.problems
