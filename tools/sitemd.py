"""Markdown for soma-lang.dev: executable sources and an HTML renderer.

Two jobs, both small and dependency-free (the build runs on a bare python3):

execute(md, soma, split="## ")
    Runs the fenced blocks a page source asks for and returns the markdown
    with REAL outputs, plus the problems found:

        ```soma file=app.cell        written to the page's folder (any language
        ```toml file=soma.toml       tag; shown with its file name)

        ```run                       one command per line, run in that folder
        soma check app.cell          with the soma binary, shown as `$ command`
        !soma run app.cell pay e1    + what it printed. A leading `!` means the
        ```                          command must FAIL (exit != 0).

    Each heading starting with `split` begins a fresh, empty folder. A command
    whose exit status contradicts the source is a problem: a page cannot teach
    what the toolchain does not do.

render(md) -> (html, toc)
    The markdown subset the repo's documents use: ATX headings, paragraphs,
    nested lists, fenced code, tables, block quotes, rules, inline code,
    bold, italic, links, bare URLs, and raw HTML blocks (a line starting
    with `<` and the lines up to the next blank one).
"""
import html
import os
import re
import shlex
import subprocess
import tempfile

FENCE = re.compile(r"^(\s*)```(\S*)(.*)$")
ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
BASE = "https://soma-lang.dev"


def slug(text):
    text = re.sub(r"<[^>]+>|`", "", text.lower())
    return re.sub(r"[^a-z0-9]+", "-", text).strip("-") or "section"


def clean_env():
    """What a stranger's shell has: no model key, no SOMA_* overrides."""
    env = {k: v for k, v in os.environ.items() if "API_KEY" not in k and not k.startswith("SOMA_")}
    env["NO_COLOR"] = "1"
    return env


def run_cmd(cmd, cwd, soma, timeout=120):
    """(exit code, output with the folder's path removed)."""
    argv = shlex.split(cmd)
    if argv[0] == "soma":
        argv[0] = soma
    try:
        r = subprocess.run(argv, cwd=cwd, env=clean_env(), stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, text=True, timeout=timeout)
        code, out = r.returncode, r.stdout
    except subprocess.TimeoutExpired:
        code, out = -1, f"(timed out after {timeout}s)"
    out = ANSI.sub("", out)
    real = os.path.realpath(cwd)
    for p in (real + os.sep, cwd + os.sep):
        out = out.replace(p, "")
    out = out.replace(real, ".").replace(cwd, ".")
    return code, out.rstrip()


def execute(md, soma, split="## ", where="page"):
    lines = md.splitlines()
    out, problems = [], []
    tmp = tempfile.TemporaryDirectory()
    folder = os.path.join(tmp.name, "0")
    os.makedirs(folder)
    count = 0
    i = 0
    while i < len(lines):
        line = lines[i]
        if split and line.startswith(split):
            count += 1
            folder = os.path.join(tmp.name, str(count))
            os.makedirs(folder)
        m = FENCE.match(line)
        if not m or m.group(1):
            out.append(line)
            i += 1
            continue
        lang, rest = m.group(2), m.group(3)
        body = []
        i += 1
        while i < len(lines) and not lines[i].startswith("```"):
            body.append(lines[i])
            i += 1
        i += 1
        file = re.search(r"file=(\S+)", rest)
        if file:
            path = os.path.join(folder, file.group(1))
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
                code, text = run_cmd(cmd, folder, soma)
                if (code != 0) != must_fail:
                    problems.append(f"{where} §{count}: `{cmd}` exited {code} (the page says it "
                                    f"{'fails' if must_fail else 'passes'})\n{text}")
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


# ── rendering ────────────────────────────────────────────────────────

def inline(text, links=None):
    """Inline markdown. `links` maps a code span's text to a URL (for
    linking builtin names and diagnostic codes automatically)."""
    stash = []

    def keep(s):
        stash.append(s)
        return f"\x00{len(stash) - 1}\x00"

    def code(m):
        c = m.group(2).strip() if len(m.group(1)) > 1 else m.group(2)
        inner = f"<code>{html.escape(c)}</code>"
        url = links(c) if links else None
        return keep(f'<a href="{url}">{inner}</a>' if url else inner)

    # code spans first: nothing inside them is markdown
    p = re.sub(r"(`+)(.+?)\1", code, text)
    p = html.escape(p, quote=False)
    p = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)",
               lambda m: keep(f'<a href="{local(m.group(2))}">{m.group(1)}</a>'), p)
    p = re.sub(r"(?<![\"'=\w])https?://[^\s<>()\x00]+[^\s<>().,;:!?'\"\x00]",
               lambda m: keep(f'<a href="{local(m.group(0))}">{m.group(0)}</a>'), p)
    p = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", p)
    p = re.sub(r"(?<![\w*])\*(?![\s*])(.+?)(?<![\s*])\*(?![\w*])", r"<em>\1</em>", p)
    p = re.sub(r"(?<![\w_])_(?![\s_])([^_\x00]+?)(?<![\s_])_(?![\w_])", r"<em>\1</em>", p)
    # restore (links may hold code spans: twice)
    for _ in range(2):
        p = re.sub(r"\x00(\d+)\x00", lambda m: stash[int(m.group(1))], p)
    return p


# a raw document that has an HTML page: link the page
AS_PAGE = {
    "/docs/guarantees.md": "/guide/guarantees", "/docs/serving.md": "/guide/serving",
    "/docs/operations.md": "/guide/operations", "/docs/cluster.md": "/guide/cluster",
    "/docs/robotics.md": "/guide/robotics", "/docs/reference.md": "/reference/",
    "/docs/builtins.md": "/builtins/", "/docs/gotchas.md": "/gotchas/", "/CHANGELOG.md": "/changelog/",
    "/corpus/index.md": "/programs/", "/learn": "/learn/",
}


def local(url):
    """Links into this site become site-relative, and to the HTML page of a
    document when it has one."""
    if url.startswith(BASE):
        url = url[len(BASE):] or "/"
    path, hash_, frag = url.partition("#")
    if re.fullmatch(r"[\w-]+\.md", path) and "/docs/" + path in AS_PAGE:
        path = "/docs/" + path  # a link between two docs/ files
    return AS_PAGE.get(path, path) + hash_ + frag


def code_block(lang, rest, code, judge=None):
    """A code block as crawlers keep it: a <div>, a caption paragraph, and
    <pre><code> (text extractors drop <figure>; a <pre> without <code> loses
    its line breaks). `judge(lang, code)` may return a sentence, as HTML,
    saying what the toolchain thinks of the block."""
    file = re.search(r"file=(\S+)", rest)
    caption = f'<p class="file">{html.escape(file.group(1))}</p>' if file else ""
    verdict = judge(lang, code) if judge and not file else None
    if lang == "console":
        return f'<div class="code out">{caption}<pre><code class="language-console">{html.escape(code)}</code></pre></div>'
    cls = f' class="language-{html.escape(lang)}"' if lang else ""
    return (f'{verdict or ""}<div class="code src">{caption}<pre><code{cls}>{html.escape(code)}</code></pre></div>')


def render(md, links=None, shift=0, anchors=True, judge=None):
    """(html, toc) — toc lists the h2 headings as (anchor, text)."""
    lines = md.splitlines()
    out, toc, para = [], [], []
    stack = []  # open lists: (indent, tag)
    used = set()

    def close_lists(indent=-1):
        while stack and stack[-1][0] > indent:
            out.append(f"</li></{stack.pop()[1]}>")

    def flush():
        if para:
            out.append(f"<p>{inline(' '.join(para), links)}</p>")
            para.clear()

    i = 0
    while i < len(lines):
        line = lines[i]
        m = FENCE.match(line)
        if m:
            flush()
            indent = len(m.group(1))
            if not (stack and indent > stack[-1][0]):
                close_lists()
            code = []
            i += 1
            while i < len(lines) and not lines[i].strip().startswith("```"):
                code.append(lines[i][indent:] if lines[i][:indent].strip() == "" else lines[i])
                i += 1
            i += 1
            out.append(code_block(m.group(2), m.group(3), "\n".join(code), judge))
            continue
        stripped = line.strip()
        if not stripped:
            flush()
            # a blank line ends a list unless the next line continues it
            j = i + 1
            while j < len(lines) and not lines[j].strip():
                j += 1
            if stack and (j >= len(lines) or not (lines[j].startswith(" ") or re.match(r"\s*([-*+]|\d+\.)\s", lines[j]))):
                close_lists()
            i += 1
            continue
        h = re.match(r"^(#{1,6})\s+(.*?)\s*#*$", line)
        if h:
            flush()
            close_lists()
            level = min(6, len(h.group(1)) + shift)
            text = h.group(2)
            anchor = slug(text)
            n = 2
            while anchor in used:
                anchor = f"{slug(text)}-{n}"
                n += 1
            used.add(anchor)
            if level == 2:
                toc.append((anchor, text))
            aid = f' id="{anchor}"' if anchors else ""
            out.append(f"<h{level}{aid}>{inline(text, links)}</h{level}>")
            i += 1
            continue
        if re.fullmatch(r"(-{3,}|\*{3,}|_{3,})", stripped):
            flush()
            close_lists()
            out.append("<hr>")
            i += 1
            continue
        if stripped.startswith("|") and i + 1 < len(lines) and re.fullmatch(r"\|?[\s:|-]+\|?", lines[i + 1].strip()) and "-" in lines[i + 1]:
            flush()
            close_lists()

            def cells(row):
                row = row.strip()
                if row.startswith("|"):
                    row = row[1:]
                if row.endswith("|") and not row.endswith("\\|"):
                    row = row[:-1]
                return [c.strip().replace("\\|", "|") for c in re.split(r"(?<!\\)\|", row)]

            head = cells(line)
            rows = []
            i += 2
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append(cells(lines[i]))
                i += 1
            t = ["<table><thead><tr>" + "".join(f"<th>{inline(c, links)}</th>" for c in head) + "</tr></thead><tbody>"]
            for r in rows:
                t.append("<tr>" + "".join(f"<td>{inline(c, links)}</td>" for c in r) + "</tr>")
            t.append("</tbody></table>")
            out.append('<div class="table">' + "".join(t) + "</div>")
            continue
        if stripped.startswith(">"):
            flush()
            close_lists()
            quote = []
            while i < len(lines) and lines[i].strip().startswith(">"):
                quote.append(re.sub(r"^\s*>\s?", "", lines[i]))
                i += 1
            inner, _ = render("\n".join(quote), links, shift, anchors=False, judge=judge)
            out.append(f"<blockquote>{inner}</blockquote>")
            continue
        if stripped.startswith("<") and not stack and not para:
            block = []
            while i < len(lines) and lines[i].strip():
                block.append(lines[i])
                i += 1
            out.append("\n".join(block))
            continue
        li = re.match(r"^(\s*)([-*+]|\d+[.)])\s+(.*)$", line)
        if li:
            flush()
            indent = len(li.group(1))
            tag = "ol" if li.group(2)[0].isdigit() else "ul"
            if stack and indent > stack[-1][0]:
                out.append(f"<{tag}><li>")
                stack.append((indent, tag))
            else:
                close_lists(indent)
                if stack and stack[-1][0] == indent:
                    out.append("</li><li>")
                else:
                    out.append(f"<{tag}><li>")
                    stack.append((indent, tag))
            para.append(li.group(3))
            i += 1
            continue
        if stack and not line.startswith(" ") and not para:
            close_lists()
        para.append(stripped)
        i += 1
    flush()
    close_lists()
    return "\n".join(out), toc


def text_of(md, limit=300):
    """The first paragraph of a markdown text, as plain text (for meta
    descriptions)."""
    for block in re.split(r"\n\s*\n", md):
        b = block.strip()
        if not b or b.startswith(("#", "```", "|", "<", "-", ">", "*")):
            continue
        t = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", b)
        t = re.sub(r"[`*_]", "", t)
        t = re.sub(r"\s+", " ", t).strip()
        if len(t) > limit:
            t = t[:limit].rsplit(" ", 1)[0] + "…"
        return t
    return ""
