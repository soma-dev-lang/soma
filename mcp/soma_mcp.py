"""
Soma MCP Server — exposes the Soma compiler as tools for AI agents.

The agent loop:
  1. soma_generate(prompt) → writes .cell file
  2. soma_check(file) → {"passed": true/false, "errors": [...]}
  3. soma_verify(file) → state machine proofs, counter-examples
  4. soma_describe(file) → structured cell description
  5. soma_serve(file, port) → starts the server

Install: pip install mcp
Run: python soma_mcp.py
Or add to claude_desktop_config.json / claude code MCP settings.
"""

import asyncio
import json
import os
import subprocess
import signal
from pathlib import Path
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("soma", instructions="""
Soma is a fractal, declarative language for building verified distributed systems.
Use these tools to generate, check, verify, describe, and serve Soma programs.
The workflow: generate → check → verify → serve. Each step is a verification gate.
""")

SOMA = os.environ.get("SOMA_BIN", "soma")
WORKDIR = os.environ.get("SOMA_WORKDIR", os.getcwd())

def run_soma(*args, cwd=None, json_mode=False) -> str:
    """Run a soma CLI command and return stdout."""
    try:
        result = subprocess.run(
            [SOMA] + list(args),
            capture_output=True, text=True, timeout=30,
            cwd=cwd or WORKDIR
        )
        # In JSON mode, only return stdout (stderr has human-readable info)
        if json_mode:
            return result.stdout.strip()
        output = result.stdout
        if result.stderr:
            output += "\n" + result.stderr
        return output.strip()
    except FileNotFoundError:
        return json.dumps({"error": f"soma binary not found at '{SOMA}'. Set SOMA_BIN env var."})
    except subprocess.TimeoutExpired:
        return json.dumps({"error": "soma command timed out (30s)"})


@mcp.tool()
def soma_describe(file: str) -> str:
    """Describe a .cell file: returns structured JSON with signals, memory, state machines, scale config, routes.
    Use this to understand what a cell does before modifying it."""
    return run_soma("describe", file)


@mcp.tool()
def soma_check(file: str) -> str:
    """Check a .cell file for compile errors. Returns JSON with passed/failed, errors, warnings.
    Always run this after generating or modifying a .cell file."""
    return run_soma("check", file, "--json", json_mode=True)


@mcp.tool()
def soma_verify(file: str) -> str:
    """Verify state machines and distribution properties. Returns JSON with temporal proofs and counter-examples.
    Run this after check passes. Counter-examples tell you exactly what's wrong."""
    return run_soma("verify", file, "--json", json_mode=True)


@mcp.tool()
def soma_generate(file: str, code: str) -> str:
    """Write a .cell file to disk. The code should be valid Soma.
    After generating, always run soma_check then soma_verify."""
    path = Path(WORKDIR) / file
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(code)
    # Auto-check
    check_result = run_soma("check", file, "--json", json_mode=True)
    return json.dumps({
        "written": str(path),
        "bytes": len(code),
        "check": json.loads(check_result) if check_result.startswith("{") else {"raw": check_result}
    }, indent=2)


@mcp.tool()
def soma_generate_toml(file: str, content: str) -> str:
    """Write a soma.toml file for verification properties and project config."""
    path = Path(WORKDIR) / file
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    return json.dumps({"written": str(path), "bytes": len(content)})


_serve_process = None

@mcp.tool()
def soma_serve(file: str, port: int = 8080) -> str:
    """Start serving a .cell file as a web application. Returns the URL.
    The server runs in the background. Use soma_stop to stop it."""
    global _serve_process
    if _serve_process and _serve_process.poll() is None:
        _serve_process.terminate()
        _serve_process.wait(timeout=5)

    _serve_process = subprocess.Popen(
        [SOMA, "serve", file, "-p", str(port)],
        cwd=WORKDIR,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    # Wait briefly for startup
    import time
    time.sleep(2)

    if _serve_process.poll() is not None:
        stderr = _serve_process.stderr.read().decode() if _serve_process.stderr else ""
        return json.dumps({"error": f"server failed to start: {stderr}"})

    return json.dumps({
        "status": "running",
        "url": f"http://localhost:{port}",
        "pid": _serve_process.pid,
    })


@mcp.tool()
def soma_stop() -> str:
    """Stop the running soma server."""
    global _serve_process
    if _serve_process and _serve_process.poll() is None:
        _serve_process.terminate()
        _serve_process.wait(timeout=5)
        _serve_process = None
        return json.dumps({"status": "stopped"})
    return json.dumps({"status": "no server running"})


@mcp.tool()
def soma_test(file: str) -> str:
    """Run tests in a .cell file. Returns pass/fail results."""
    return run_soma("test", file)


@mcp.tool()
def soma_run(file: str, *args: str) -> str:
    """Run a .cell file with arguments. Returns the handler result."""
    return run_soma("run", file, *args)


@mcp.resource("soma://reference")
def soma_reference() -> str:
    """The Soma language reference — all syntax, builtins, and patterns.
    Read this before generating any Soma code."""
    ref_path = Path(__file__).parent.parent / "SOMA_REFERENCE.md"
    if ref_path.exists():
        return ref_path.read_text()
    return "SOMA_REFERENCE.md not found. Check the soma installation."


if __name__ == "__main__":
    mcp.run()
