"""
Soma Storage Provider — Reference Sidecar (in-memory)

Usage:
  python tools/sidecar/server.py
  python tools/sidecar/server.py --port 9100

This implements the Soma storage provider HTTP protocol.
Any Soma app can use this as a backend by setting:

  [storage]
  provider = "sidecar"
  [storage.config]
  url = "http://localhost:9100"
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import sys

storage = {}  # "cell.field" -> dict of key -> StoredValue
logs = {}     # "cell.field" -> list of StoredValue

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/health':
            tables = len(storage)
            total_keys = sum(len(v) for v in storage.values())
            self.respond({'status': 'ok', 'provider': 'memory-sidecar', 'tables': tables, 'keys': total_keys})
        else:
            self.respond({'error': 'use POST'}, status=405)

    def do_POST(self):
        content_len = int(self.headers.get('Content-Length', 0))
        body = json.loads(self.rfile.read(content_len)) if content_len > 0 else {}
        cell = body.get('cell', '')
        field = body.get('field', '')
        ns = f"{cell}.{field}"

        if ns not in storage:
            storage[ns] = {}
        if ns not in logs:
            logs[ns] = []

        path = self.path

        if path == '/get':
            value = storage[ns].get(body['key'])
            self.respond({'value': value})

        elif path == '/set':
            storage[ns][body['key']] = body['value']
            self.respond({'ok': True})

        elif path == '/delete':
            deleted = body['key'] in storage[ns]
            storage[ns].pop(body['key'], None)
            self.respond({'deleted': deleted})

        elif path == '/keys':
            keys = [k for k in storage[ns].keys() if not k.startswith('__')]
            self.respond({'keys': keys})

        elif path == '/values':
            self.respond({'values': list(storage[ns].values())})

        elif path == '/has':
            self.respond({'exists': body['key'] in storage[ns]})

        elif path == '/len':
            count = len([k for k in storage[ns] if not k.startswith('__')])
            self.respond({'len': count + len(logs[ns])})

        elif path == '/append':
            logs[ns].append(body['value'])
            self.respond({'ok': True})

        elif path == '/list':
            self.respond({'items': logs[ns]})

        elif path == '/health':
            tables = len(storage)
            total_keys = sum(len(v) for v in storage.values())
            self.respond({'status': 'ok', 'provider': 'memory-sidecar', 'tables': tables, 'keys': total_keys})

        else:
            self.respond({'error': f'unknown endpoint: {path}'}, status=404)

    def respond(self, data, status=200):
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, format, *args):
        print(f"  {args[0]}")

if __name__ == '__main__':
    port = 9100
    for i, arg in enumerate(sys.argv):
        if arg == '--port' and i + 1 < len(sys.argv):
            port = int(sys.argv[i + 1])

    print(f"soma storage sidecar (memory)")
    print(f"listening on http://localhost:{port}")
    print(f"---")
    HTTPServer(('0.0.0.0', port), Handler).serve_forever()
