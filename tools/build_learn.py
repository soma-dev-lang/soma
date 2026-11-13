#!/usr/bin/env python3
"""Build the course (docs/site/learn.md) into site/llms-learn.txt and
site/learn.html, with REAL outputs.

The source is markdown with two kinds of fenced blocks the build acts on:

    ```soma file=app.cell        written to the lesson's folder (any language
    ```toml file=soma.toml       tag; shown with its file name)

    ```run                       one command per line, run in the lesson's
    soma check app.cell          folder with the soma binary; shown as
    !soma run app.cell pay e1    `$ command` + what it printed. A leading `!`
    ```                          means the command must FAIL (exit != 0).

Each `## ` heading starts a lesson in a new, empty folder. A command whose
exit status contradicts the source (fails without `!`, passes with it) is a
build error: the course cannot teach what the toolchain does not do.

    SOMA=compiler/target/release/soma python3 tools/build_learn.py
"""
import html
import os
import re
import shlex
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SITE = os.path.join(ROOT, "site")
SOURCE = os.path.join(ROOT, "docs", "site", "learn.md")
FENCE = re.compile(r"^```(\S*)(.*)$")


def run_course(soma):
    """The course as markdown with outputs filled in, and the problems found."""
    lines = open(SOURCE, encoding="utf-8").read().splitlines()
    env = {k: v for k, v in os.environ.items()
           if "API_KEY" not in k and not k.startswith("SOMA_")}
    out, problems = [], []
    tmp = tempfile.TemporaryDirectory()
    lesson = tmp.name
    count = 0
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("## "):
            count += 1
            lesson = os.path.join(tmp.name, f"lesson{count}")
            os.makedirs(lesson)
        m = FENCE.match(line)
        if not m:
            out.append(line)
            i += 1
            continue
        lang, rest = m.group(1), m.group(2)
        body = []
        i += 1
        while i < len(lines) and not lines[i].startswith("```"):
            body.append(lines[i])
            i += 1
        i += 1  # the closing fence
        file = re.search(r"file=(\S+)", rest)
        if file:
            path = os.path.join(lesson, file.group(1))
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as f:
                f.write("\n".join(body) + "\n")
            out.append(f"```{lang} file={file.group(1)}")
            out.extend(body)
            out.append("```")
        elif lang == "run":
            shown = []
            for cmd in body:
                if not cmd.strip():
                    continue
                must_fail = cmd.startswith("!")
                cmd = cmd.lstrip("!")
                argv = shlex.split(cmd)
                if argv[0] == "soma":
                    argv[0] = soma
                r = subprocess.run(argv, cwd=lesson, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, text=True, timeout=120)
                if (r.returncode != 0) != must_fail:
                    problems.append(f"lesson {count}: `{cmd}` exited {r.returncode} "
                                    f"(the course says it {'fails' if must_fail else 'passes'})\n"
                                    + r.stdout.rstrip())
                text = r.stdout.replace(lesson + os.sep, "").replace(lesson, ".").rstrip()
                shown.append(f"$ {cmd}" + ("\n" + text if text else ""))
            out.append("```console")
            out.append("\n\n".join(shown))
            out.append("```")
        else:
            out.append(f"```{lang}{rest}")
            out.extend(body)
            out.append("```")
    tmp.cleanup()
    return "\n".join(out) + "\n", problems


# ── markdown → html (the subset the course uses) ─────────────────────

def inline(text):
    parts = re.split(r"(`[^`]+`)", text)
    res = []
    for p in parts:
        if p.startswith("`") and p.endswith("`") and len(p) > 1:
            res.append(f"<code>{html.escape(p[1:-1])}</code>")
            continue
        p = html.escape(p, quote=False)
        p = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", p)
        p = re.sub(r"(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?![\w*])", r"<em>\1</em>", p)
        p = re.sub(r"(https://soma-lang\.dev)(/[\w./-]*)?",
                   lambda m: f'<a href="{m.group(2) or "/"}">{m.group(0)}</a>', p)
        res.append(p)
    return "".join(res)


def to_html(md):
    lines = md.splitlines()
    body, para, items = [], [], []
    toc = []

    def flush():
        if para:
            body.append(f"<p>{inline(' '.join(para))}</p>")
            para.clear()
        if items:
            body.append("<ul>" + "".join(f"<li>{inline(x)}</li>" for x in items) + "</ul>")
            items.clear()

    i = 0
    while i < len(lines):
        line = lines[i]
        m = FENCE.match(line)
        if m:
            flush()
            lang, rest = m.group(1), m.group(2)
            code = []
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                code.append(lines[i])
                i += 1
            i += 1
            file = re.search(r"file=(\S+)", rest)
            caption = f'<div class="file">{html.escape(file.group(1))}</div>' if file else ""
            cls = "out" if lang == "console" else "src"
            body.append(f'<figure class="{cls}">{caption}<pre><code>{html.escape(chr(10).join(code))}</code></pre></figure>')
            continue
        if line.startswith("# "):
            flush()
            body.append(f"<h1>{inline(line[2:])}</h1>")
        elif line.startswith("## "):
            flush()
            title = line[3:]
            anchor = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
            toc.append((anchor, title))
            body.append(f'<h2 id="{anchor}">{inline(title)}</h2>')
        elif line.startswith("- "):
            if para:
                flush()
            items.append(line[2:])
        elif line.startswith("  ") and items and line.strip():
            items[-1] += " " + line.strip()
        elif not line.strip():
            flush()
        else:
            if items:
                flush()
            para.append(line.strip())
        i += 1
    flush()
    nav = "".join(f'<li><a href="#{a}">{inline(t)}</a></li>' for a, t in toc)
    return "\n".join(body), nav


PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Learn Soma</title>
<meta name="description" content="Six lessons for language models and the people who run them: a cell, errors that name their fix, a proven limit, a checked lifecycle, an agent tested offline, a large program written skeleton first. Every output is real, generated by the released soma binary.">
<link rel="canonical" href="https://soma-lang.dev/learn">
<link rel="alternate" type="text/plain" href="/llms-learn.txt" title="This course as plain text for models">
<link rel="alternate" type="text/plain" href="/llms.txt" title="Language summary for models">
<style>
:root { color-scheme: light dark; --bg:#0f1115; --panel:#171a21; --panel-2:#1f232c; --text:#e7e9ee; --muted:#a9afbd; --line:#303642; --accent:#8ab4f8; --ok:#7bd88f; --code:#d9e2f1; }
@media (prefers-color-scheme: light) { :root:not([data-theme="dark"]) { --bg:#fbfbfc; --panel:#f1f3f6; --panel-2:#e7eaf0; --text:#15181e; --muted:#454c59; --line:#d6dae2; --accent:#1f5fbf; --ok:#1d7a35; --code:#1b2230; } }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--text); font:16px/1.65 system-ui,-apple-system,"Segoe UI",sans-serif; }
a { color:var(--accent); text-decoration:none; } a:hover { text-decoration:underline; }
code, pre { font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace; color:var(--code); }
code { background:var(--panel-2); padding:.08rem .3rem; border-radius:4px; font-size:.92em; }
nav.top { display:flex; gap:1.2rem; align-items:center; padding:1rem 1.5rem; border-bottom:1px solid var(--line); flex-wrap:wrap; }
nav.top .brand { font-weight:700; letter-spacing:.04em; margin-right:auto; color:var(--text); }
.wrap { display:grid; grid-template-columns:220px minmax(0,1fr); gap:2.5rem; max-width:1100px; margin:0 auto; padding:2rem 1.4rem 4rem; }
aside { position:sticky; top:1rem; align-self:start; font-size:.9rem; }
aside ul { list-style:none; padding:0; margin:0; }
aside li { margin:.45rem 0; }
aside a { color:var(--muted); }
main { min-width:0; }
h1 { font-size:2.2rem; line-height:1.15; margin:0 0 1rem; }
h2 { margin:2.8rem 0 .8rem; font-size:1.4rem; border-top:1px solid var(--line); padding-top:1.6rem; }
p, li { color:var(--muted); } strong { color:var(--text); }
figure { margin:1rem 0; }
.file { font:600 .78rem ui-monospace,Menlo,monospace; color:var(--muted); padding:.35rem .9rem; background:var(--panel-2); border:1px solid var(--line); border-bottom:none; border-radius:8px 8px 0 0; display:inline-block; }
pre { margin:0; padding:1rem; overflow-x:auto; background:var(--panel); border:1px solid var(--line); border-radius:0 8px 8px 8px; line-height:1.45; font-size:13.5px; }
figure.src pre { border-radius:8px; } figure.src .file + pre { border-radius:0 8px 8px 8px; }
figure.out pre { border-left:3px solid var(--ok); border-radius:8px; }
pre code { background:none; padding:0; font-size:inherit; }
.note { font-size:.9rem; color:var(--muted); border:1px solid var(--line); border-radius:8px; padding:.8rem 1rem; background:var(--panel); }
footer { margin-top:3rem; color:var(--muted); font-size:.9rem; border-top:1px solid var(--line); padding-top:1.2rem; }
@media (max-width: 820px) { .wrap { grid-template-columns:1fr; gap:0; padding:1.2rem 16px 3rem; } aside { position:static; margin-bottom:1rem; } }
</style>
</head>
<body>
<nav class="top">
  <a class="brand" href="/">soma</a>
  <a href="/learn">Learn</a>
  <a href="/agents">For agents</a>
  <a href="/llms.txt">llms.txt</a>
  <a href="https://github.com/soma-dev-lang/soma">GitHub</a>
</nav>
<div class="wrap">
<aside><ul>{nav}</ul><p><a href="/llms-learn.txt">Plain text for models</a></p></aside>
<main>
<p class="note">Generated by <code>soma {version}</code> on {date}: each program was written to an empty folder and each command run; the green blocks are what it printed.</p>
{body}
<footer>The same course as plain text: <a href="/llms-learn.txt">/llms-learn.txt</a>. Source: <a href="https://github.com/soma-dev-lang/soma/blob/main/docs/site/learn.md">docs/site/learn.md</a>.</footer>
</main>
</div>
</body>
</html>
"""


def build(soma, version, date):
    md, problems = run_course(soma)
    header = (f"<!-- Generated by tools/build_learn.py from docs/site/learn.md with soma {version}: "
              "every output below was printed by that binary. -->\n\n")
    with open(os.path.join(SITE, "llms-learn.txt"), "w", encoding="utf-8") as f:
        f.write(header + md)
    body, nav = to_html(md)
    page = PAGE.replace("{nav}", nav).replace("{body}", body).replace("{version}", html.escape(version)).replace("{date}", date)
    with open(os.path.join(SITE, "learn.html"), "w", encoding="utf-8") as f:
        f.write(page)
    return problems


if __name__ == "__main__":
    import datetime
    soma = os.path.abspath(os.environ.get("SOMA", "soma")) if os.sep in os.environ.get("SOMA", "soma") else os.environ.get("SOMA", "soma")
    version = subprocess.run([soma, "--version"], capture_output=True, text=True).stdout.split()[-1]
    problems = build(soma, version, datetime.date.today().isoformat())
    for p in problems:
        print("COURSE BROKEN —", p)
    sys.exit(1 if problems else 0)
