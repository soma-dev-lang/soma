// soma-ui.js — reusable components for Soma dashboards
// Usage:
//   SomaTable('#prices', { url: '/api/prices', cols: [...], key: 'symbol', flash: 'mid' })
//   SomaKPI('#pnl', { label: 'P&L', value: () => computedValue, signed: true })
//   SomaApp({ apis: {prices: '/api/prices', ...}, refresh: 2000, onUpdate: fn })

const SomaUI = (() => {
    const _prev = {};
    const _intervals = [];

    // ── Styles (injected once) ──
    const CSS = `
    :root{--bg:#0b0e11;--surface:#141821;--border:#1e2530;--text:#8a92a0;--bright:#eaecef;--dim:#4a5568;--gold:#f0b90b;--up:#0ecb81;--dn:#f6465d;--mono:'SF Mono','Fira Code','Cascadia Code',monospace}
    *{margin:0;padding:0;box-sizing:border-box}
    body{background:var(--bg);color:var(--text);font-family:'Helvetica Neue',Arial,sans-serif;font-size:13px}
    .soma-wrap{max-width:1400px;margin:0 auto;padding:10px 16px}
    .soma-topbar{display:flex;align-items:center;justify-content:space-between;padding:8px 0;border-bottom:1px solid var(--border);margin-bottom:10px}
    .soma-brand{font-size:13px;font-weight:800;color:var(--gold);letter-spacing:3px}
    .soma-status{display:flex;align-items:center;gap:6px;font-size:11px;color:var(--dim)}
    .soma-dot{width:5px;height:5px;border-radius:50%;background:var(--up);animation:soma-blink 2s infinite}
    @keyframes soma-blink{0%,100%{opacity:1}50%{opacity:.2}}
    .soma-kpi-row{display:flex;gap:2px;margin-bottom:10px}
    .soma-kpi{flex:1;background:var(--surface);padding:10px 14px}
    .soma-kpi:first-child{border-radius:4px 0 0 4px}
    .soma-kpi:last-child{border-radius:0 4px 4px 0}
    .soma-kpi-label{font-size:9px;text-transform:uppercase;letter-spacing:1.5px;color:var(--dim);margin-bottom:2px}
    .soma-kpi-val{font-size:20px;font-weight:800;font-family:var(--mono);color:var(--bright);transition:color .3s,background .5s}
    .soma-panel{background:var(--surface);border-radius:4px;overflow:hidden;margin-bottom:8px}
    .soma-panel-head{padding:8px 14px;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:2px;color:var(--dim);border-bottom:1px solid var(--border);display:flex;justify-content:space-between}
    .soma-panel-head .ct{color:var(--gold);font-family:var(--mono)}
    .soma-grid{display:grid;gap:8px}
    .soma-g2{grid-template-columns:1fr 1fr}
    .soma-table{width:100%;border-collapse:collapse}
    .soma-table th{text-align:left;padding:5px 14px;font-size:9px;text-transform:uppercase;letter-spacing:1px;color:var(--dim);font-weight:600}
    .soma-table th.r{text-align:right}
    .soma-table td{padding:6px 14px;font-size:12px;color:var(--bright);border-bottom:1px solid rgba(30,37,48,.4);font-family:var(--mono);transition:color .3s,background .5s}
    .soma-table td.r{text-align:right}
    .soma-table td.sym{font-weight:700;color:var(--gold);font-size:11px;letter-spacing:.5px}
    .soma-table td.lbl{font-family:'Helvetica Neue',Arial,sans-serif}
    .soma-table tr:hover td{background:rgba(240,185,11,.03)}
    .soma-table td.empty{text-align:center;padding:20px;color:var(--dim);font-family:'Helvetica Neue',sans-serif}
    .tick-up{background:rgba(14,203,129,.18) !important;color:var(--up) !important}
    .tick-dn{background:rgba(246,70,93,.18) !important;color:var(--dn) !important}
    .soma-kpi-val.tick-up{background:rgba(14,203,129,.15);border-radius:4px}
    .soma-kpi-val.tick-dn{background:rgba(246,70,93,.15);border-radius:4px}
    .up{color:var(--up)}.dn{color:var(--dn)}
    .buy{color:var(--up);font-weight:700;font-size:10px}.sell{color:var(--dn);font-weight:700;font-size:10px}
    .badge{display:inline-block;padding:1px 6px;border-radius:2px;font-size:9px;font-weight:700;letter-spacing:.5px;text-transform:uppercase}
    .b-fill{background:rgba(14,203,129,.1);color:var(--up)}
    .b-rej{background:rgba(246,70,93,.1);color:var(--dn)}
    .b-pend{background:rgba(240,185,11,.1);color:var(--gold)}
    .soma-footer{text-align:center;padding:12px 0;color:#2a2f38;font-size:9px;letter-spacing:2px;text-transform:uppercase;margin-top:6px}
    `;

    let _stylesInjected = false;
    function injectStyles() {
        if (_stylesInjected) return;
        const s = document.createElement('style');
        s.textContent = CSS;
        document.head.appendChild(s);
        _stylesInjected = true;
    }

    const fmt = n => Number(n).toLocaleString('en-US', {maximumFractionDigits: 2});

    function flash(el, dir) {
        const cls = dir === 'up' ? 'tick-up' : 'tick-dn';
        el.classList.add(cls);
        setTimeout(() => el.classList.remove(cls), 600);
    }

    function setVal(id, val, signed) {
        const el = document.getElementById(id);
        if (!el) return;
        const num = parseFloat(String(val).replace(/[+,]/g, ''));
        const old = _prev[id] !== undefined ? parseFloat(String(_prev[id]).replace(/[+,]/g, '')) : undefined;

        let text = String(val);
        if (signed && num >= 0) text = '+' + val;
        el.textContent = text;

        if (signed) {
            el.classList.remove('up', 'dn');
            el.classList.add(num >= 0 ? 'up' : 'dn');
        }
        if (old !== undefined && old !== num && !isNaN(num) && !isNaN(old)) {
            flash(el, num > old ? 'up' : 'dn');
        }
        _prev[id] = val;
    }

    // ── SomaKPI ──
    // SomaKPI(selector, { label, id, signed })
    function KPI(sel, opts) {
        injectStyles();
        const el = document.querySelector(sel);
        const id = opts.id || 'kpi-' + Math.random().toString(36).substr(2, 6);
        el.innerHTML = `<div class="soma-kpi"><div class="soma-kpi-label">${opts.label}</div><div class="soma-kpi-val" id="${id}">—</div></div>`;
        return {
            set(val) { setVal(id, val, opts.signed); },
            el: document.getElementById(id),
            id
        };
    }

    // ── SomaTable ──
    // SomaTable(selector, { title, cols: [{key, label, align, format, flash, class}], key, emptyText })
    function Table(sel, opts) {
        injectStyles();
        const el = document.querySelector(sel);
        const tid = 'st-' + Math.random().toString(36).substr(2, 6);
        const ths = opts.cols.map(c =>
            `<th${c.align === 'right' ? ' class="r"' : ''}>${c.label || c.key}</th>`
        ).join('');
        el.innerHTML = `<div class="soma-panel">` +
            (opts.title ? `<div class="soma-panel-head">${opts.title} <span class="ct" id="${tid}-ct"></span></div>` : '') +
            `<table class="soma-table"><thead><tr>${ths}</tr></thead><tbody id="${tid}"></tbody></table></div>`;

        const tbody = document.getElementById(tid);

        return {
            update(rows) {
                // Ensure rows exist
                for (const row of rows) {
                    const rk = row[opts.key] || '';
                    const rid = tid + '-' + rk;
                    let tr = document.getElementById(rid);
                    if (!tr) {
                        tr = document.createElement('tr');
                        tr.id = rid;
                        opts.cols.forEach((c, i) => {
                            const td = document.createElement('td');
                            td.id = rid + '-' + i;
                            tr.appendChild(td);
                        });
                        tbody.appendChild(tr);
                    }
                    opts.cols.forEach((c, i) => {
                        const td = tr.children[i];
                        let val = c.compute ? c.compute(row) : (row[c.key] || '');
                        if (c.format) val = c.format(val, row);

                        td.className = '';
                        if (c.align === 'right') td.classList.add('r');
                        if (c.class) {
                            const cls = typeof c.class === 'function' ? c.class(val, row) : c.class;
                            if (cls) cls.split(' ').forEach(x => td.classList.add(x));
                        }

                        if (c.flash) {
                            setVal(td.id, val, c.signed);
                        } else if (c.html || (typeof val === 'string' && val.includes('<'))) {
                            td.innerHTML = val;
                        } else {
                            td.textContent = val;
                        }
                    });
                }
                // Remove rows that no longer exist
                const keys = new Set(rows.map(r => tid + '-' + (r[opts.key] || '')));
                Array.from(tbody.children).forEach(tr => {
                    if (!keys.has(tr.id) && !tr.id.endsWith('-total')) tr.remove();
                });
                // Count
                const ct = document.getElementById(tid + '-ct');
                if (ct) ct.textContent = rows.length;
            },
            // Add a total/summary row
            setTotal(cols) {
                let tr = document.getElementById(tid + '-total');
                if (!tr) {
                    tr = document.createElement('tr');
                    tr.id = tid + '-total';
                    tr.style.borderTop = '2px solid #2a3040';
                    opts.cols.forEach((_, i) => {
                        const td = document.createElement('td');
                        td.id = tid + '-total-' + i;
                        tr.appendChild(td);
                    });
                    tbody.appendChild(tr);
                }
                opts.cols.forEach((c, i) => {
                    const td = tr.children[i];
                    const val = cols[c.key];
                    if (val !== undefined) {
                        td.className = c.align === 'right' ? 'r' : '';
                        if (c.flash || c.signed) {
                            setVal(td.id, val, c.signed);
                        } else {
                            td.textContent = val;
                        }
                        if (i === 0) { td.style.fontWeight = '700'; td.style.color = '#eaecef'; }
                    } else {
                        td.textContent = '';
                    }
                });
            },
            tbody,
            id: tid
        };
    }

    // ── SomaApp ──
    // SomaApp({ refresh, onUpdate })
    function App(opts) {
        injectStyles();
        const interval = setInterval(opts.onUpdate, opts.refresh || 2000);
        _intervals.push(interval);
        opts.onUpdate();
        return { stop: () => clearInterval(interval) };
    }

    // ── Layout helpers ──
    function Topbar(sel, opts) {
        injectStyles();
        document.querySelector(sel).innerHTML =
            `<div class="soma-topbar"><div class="soma-brand">${opts.title}</div>` +
            `<div class="soma-status"><div class="soma-dot"></div>LIVE <span id="soma-clock"></span></div></div>`;
        setInterval(() => {
            const d = new Date();
            const el = document.getElementById('soma-clock');
            if (el) el.textContent = ` ${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}:${String(d.getSeconds()).padStart(2,'0')}`;
        }, 1000);
    }

    function KPIRow(sel, kpis) {
        injectStyles();
        const el = document.querySelector(sel);
        el.innerHTML = '<div class="soma-kpi-row">' +
            kpis.map(k => `<div class="soma-kpi"><div class="soma-kpi-label">${k.label}</div><div class="soma-kpi-val" id="${k.id}">—</div></div>`).join('') +
            '</div>';
        return kpis.reduce((acc, k) => {
            acc[k.id] = { set(val) { setVal(k.id, val, k.signed); } };
            return acc;
        }, {});
    }

    function Grid(sel, cols) {
        injectStyles();
        const el = document.querySelector(sel);
        el.className = 'soma-grid soma-g' + cols;
        return el;
    }

    function Footer(sel, text) {
        injectStyles();
        document.querySelector(sel).innerHTML = `<div class="soma-footer">${text}</div>`;
    }

    return { KPI, Table, App, Topbar, KPIRow, Grid, Footer, fmt, setVal, flash };
})();
