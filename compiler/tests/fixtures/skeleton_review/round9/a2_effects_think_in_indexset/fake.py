import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
LOG = sys.argv[2]
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0)); body = json.loads(self.rfile.read(n) or b'{}')
        tools = [t.get('function', {}).get('name') for t in body.get('tools', [])]
        msgs = body.get('messages', [])
        had_tool = any(m.get('role') == 'tool' for m in msgs)
        open(LOG, 'a').write(json.dumps({'tools': tools, 'last': msgs[-1].get('content') if msgs else None, 'had_tool': had_tool}) + '\n')
        if 'refund' in tools and not (msgs and msgs[-1].get('role') == 'tool'):
            msg = {'role': 'assistant', 'content': None, 'tool_calls': [{'id': 'c1', 'type': 'function', 'function': {'name': 'refund', 'arguments': json.dumps({'id': 'k'})}}]}
            fr = 'tool_calls'
        else:
            msg = {'role': 'assistant', 'content': 'done'}; fr = 'stop'
        out = json.dumps({'id': 'x', 'object': 'chat.completion', 'choices': [{'index': 0, 'message': msg, 'finish_reason': fr}], 'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(out))); self.end_headers(); self.wfile.write(out)
HTTPServer(('127.0.0.1', int(sys.argv[1])), H).serve_forever()
