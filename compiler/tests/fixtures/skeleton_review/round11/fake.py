# Faux serveur modele OpenAI-compatible, sans etat.
# Le DERNIER message user contient un script: "CALL nom k=v k2=v2;CALL nom2 k=v;SAY texte"
# A chaque requete il compte les messages role=tool APRES ce message user et renvoie l'appel suivant.
import json, sys, re
from http.server import BaseHTTPRequestHandler, HTTPServer
LOG = sys.argv[2] if len(sys.argv) > 2 else None
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0)); body = json.loads(self.rfile.read(n) or b'{}')
        tools = [t.get('function', {}).get('name') for t in body.get('tools', [])]
        msgs = body.get('messages', [])
        ui = max([i for i, m in enumerate(msgs) if m.get('role') == 'user'] or [0])
        prompt = msgs[ui].get('content') or '' if msgs else ''
        done = sum(1 for m in msgs[ui+1:] if m.get('role') == 'tool')
        steps = [s.strip() for s in prompt.split(';') if s.strip().startswith('CALL ') or s.strip().startswith('SAY ')]
        calls = [s for s in steps if s.startswith('CALL ')]
        say = next((s[4:] for s in steps if s.startswith('SAY ')), 'done')
        if LOG: open(LOG, 'a').write(json.dumps({'tools': tools, 'prompt': prompt, 'done': done, 'last': msgs[-1] if msgs else None}) + '\n')
        if done < len(calls):
            m = re.match(r'CALL (\w+)\s*(.*)', calls[done], re.S)
            msg = {'role': 'assistant', 'content': None, 'tool_calls': [{'id': 'c%d' % done, 'type': 'function', 'function': {'name': m.group(1), 'arguments': json.dumps({k: (int(v) if re.fullmatch(r'-?\d+', v) else v) for k, v in (kv.split('=', 1) for kv in m.group(2).split())})}}]}
            fr = 'tool_calls'
        else:
            msg = {'role': 'assistant', 'content': say}; fr = 'stop'
        out = json.dumps({'id': 'x', 'object': 'chat.completion', 'choices': [{'index': 0, 'message': msg, 'finish_reason': fr}], 'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(out))); self.end_headers(); self.wfile.write(out)
HTTPServer(('127.0.0.1', int(sys.argv[1])), H).serve_forever()
