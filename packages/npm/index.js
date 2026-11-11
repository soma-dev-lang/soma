// Call a Soma program from Node. The program runs in a `soma embed` process
// beside this one; each call is one atomic handler invocation.
'use strict';
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const readline = require('readline');

const INT53 = BigInt(Number.MAX_SAFE_INTEGER);

class SomaError extends Error {
  constructor(kind, message) { super(message); this.name = 'SomaError'; this.kind = kind; }
}

class LoadError extends Error {
  constructor(message, errors) { super(message); this.name = 'LoadError'; this.errors = errors || []; }
}

class Variant {
  constructor(type, variant, fields, values) {
    this.type = type; this.variant = variant; this.fields = fields || {}; this.values = values || [];
  }
}

function encode(v) {
  if (v === null || v === undefined) return null;
  if (typeof v === 'bigint') return (v >= -INT53 && v <= INT53) ? Number(v) : { __soma_int__: v.toString() };
  if (typeof v === 'number' || typeof v === 'string' || typeof v === 'boolean') return v;
  if (Array.isArray(v)) return v.map(encode);
  if (v instanceof Variant) {
    const out = { _type: v.type, _variant: v.variant };
    if (v.values.length) out._values = v.values.map(encode);
    for (const [k, x] of Object.entries(v.fields)) out[k] = encode(x);
    return out;
  }
  if (typeof v === 'object') {
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = encode(x);
    return out;
  }
  throw new TypeError(`soma: a ${typeof v} cannot be passed to a handler`);
}

function decode(v) {
  if (Array.isArray(v)) return v.map(decode);
  if (v !== null && typeof v === 'object') {
    const keys = Object.keys(v);
    if (keys.length === 1 && typeof v.__soma_int__ === 'string') return BigInt(v.__soma_int__);
    if ('_type' in v && '_variant' in v) {
      const fields = {};
      for (const k of keys) if (k !== '_type' && k !== '_variant' && k !== '_values') fields[k] = decode(v[k]);
      return new Variant(v._type, v._variant, fields, (v._values || []).map(decode));
    }
    const out = {};
    for (const k of keys) out[k] = decode(v[k]);
    return out;
  }
  return v;
}

function binary() {
  if (process.env.SOMA_BIN) return process.env.SOMA_BIN;
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    const candidate = path.join(dir, 'soma');
    if (dir && fs.existsSync(candidate)) return candidate;
  }
  const home = path.join(os.homedir(), '.soma', 'bin', 'soma');
  if (fs.existsSync(home)) return home;
  throw new LoadError('soma: the `soma` binary was not found — install it (https://soma-lang.dev) or set SOMA_BIN');
}

function load(file, options) {
  return new Promise((resolve, reject) => {
    let child;
    try {
      child = spawn(binary(), ['embed', file], { env: { ...process.env, ...((options && options.env) || {}) }, stdio: ['pipe', 'pipe', 'inherit'] });
    } catch (e) { reject(e instanceof LoadError ? e : new LoadError(`soma: cannot start \`soma embed\`: ${e.message}`)); return; }
    const lines = readline.createInterface({ input: child.stdout });
    const waiting = [];      // one resolver per line still to come, in order
    let ready = false, closed = false, next = 0;

    const fail = (e) => { while (waiting.length) waiting.shift().reject(e); };
    child.on('error', (e) => { closed = true; const err = new LoadError(`soma: cannot start \`soma embed\`: ${e.message}`); if (!ready) reject(err); fail(err); });
    child.on('exit', () => {
      closed = true;
      if (!ready) reject(new LoadError(`soma: \`soma embed ${file}\` ended before it was ready (is the binary 3.0 or later?)`));
      fail(new SomaError('internal', 'soma: the `soma embed` process ended during the call'));
    });

    const program = {
      version: '',
      handlers() { return program._cells; },
      call(handler, args) {
        if (closed) return Promise.reject(new SomaError('closed', 'soma: the program is closed'));
        let payload;
        try { payload = Array.isArray(args) ? args.map(encode) : (args === undefined ? [] : encode(args)); }
        catch (e) { return Promise.reject(e); }
        return new Promise((res, rej) => {
          waiting.push({ resolve: res, reject: rej });
          child.stdin.write(JSON.stringify({ id: ++next, call: handler, args: payload }) + '\n');
        });
      },
      close() {
        if (closed) return Promise.resolve();
        return new Promise((res) => { child.once('exit', () => res()); child.stdin.end(); });
      },
    };

    lines.on('line', (line) => {
      let message;
      try { message = JSON.parse(line); } catch (e) { return; }
      if (!ready) {
        if (!message.ready) {
          const errors = message.errors || [];
          reject(new LoadError(`soma: ${file} fails \`soma check\` (${errors.length} error(s)): ${errors.length ? errors[0].message : ''}`, errors));
          return;
        }
        ready = true;
        program._cells = message.cells;
        program.version = message.soma;
        // handlers are methods: app.refund("o1", 40)
        resolve(new Proxy(program, {
          get(target, name) {
            if (name in target || typeof name !== 'string' || name === 'then') return target[name];
            if (Object.values(target._cells).some((handlers) => name in handlers)) return (...args) => target.call(name, args);
            return undefined;
          },
        }));
        return;
      }
      const pending = waiting.shift();
      if (!pending) return;
      if (message.ok) pending.resolve(decode(message.value));
      else pending.reject(new SomaError((message.error && message.error.kind) || 'internal', (message.error && message.error.message) || 'soma: the call failed'));
    });
  });
}

module.exports = { load, SomaError, LoadError, Variant };
