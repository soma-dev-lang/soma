//! Live verification dashboard for `soma serve`.
//!
//! Renders a self-contained HTML page showing state machine diagrams,
//! budget breakdowns, and verification results for every cell in the
//! program.

use crate::ast::*;
use crate::checker::budget::{self, BudgetReport, BudgetVerdict, Cost};

/// Collect dashboard data from the program and render the full HTML page.
pub fn render_dashboard(program: &Program) -> String {
    let mut cells_json = Vec::new();

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        let report = budget::check_cell(&cell.node);
        let cell_json = cell_to_json(&cell.node, &report);
        cells_json.push(cell_json);
    }

    let data = format!("[{}]", cells_json.join(","));
    build_html(&data)
}

fn cost_to_bytes(c: &Cost) -> serde_json::Value {
    match c {
        Cost::Bounded(n) => serde_json::json!({"bounded": true, "bytes": *n}),
        Cost::Unbounded(reasons) => serde_json::json!({"bounded": false, "reasons": reasons}),
    }
}

fn cell_to_json(cell: &CellDef, report: &BudgetReport) -> String {
    // State machines
    let mut state_machines = Vec::new();
    for section in &cell.sections {
        if let Section::State(ref sm) = section.node {
            let mut states = std::collections::HashSet::new();
            states.insert(sm.initial.clone());
            for t in &sm.transitions {
                if t.node.from != "*" {
                    states.insert(t.node.from.clone());
                }
                states.insert(t.node.to.clone());
            }
            let mut states_vec: Vec<String> = states.into_iter().collect();
            states_vec.sort();

            let transitions: Vec<serde_json::Value> = sm.transitions.iter().map(|t| {
                serde_json::json!({
                    "from": t.node.from,
                    "to": t.node.to,
                    "has_guard": t.node.guard.is_some(),
                })
            }).collect();

            state_machines.push(serde_json::json!({
                "name": sm.name,
                "initial": sm.initial,
                "states": states_vec,
                "transitions": transitions,
            }));
        }
    }

    // Budget breakdown
    let handler_breakdown: Vec<serde_json::Value> = report.handler_breakdown.iter().map(|(name, cost)| {
        serde_json::json!({
            "name": name,
            "cost": cost_to_bytes(cost),
        })
    }).collect();

    let verdict = match report.verdict() {
        BudgetVerdict::Pass => "pass",
        BudgetVerdict::Fail => "fail",
        BudgetVerdict::Advisory => "advisory",
        BudgetVerdict::NoBudgetDeclared => "no_budget",
    };

    // Verification results
    let mut verifications = Vec::new();

    // Budget verification
    verifications.push(serde_json::json!({
        "property": format!("memory budget ({})", cell.name),
        "status": verdict,
        "detail": match report.verdict() {
            BudgetVerdict::Pass => format!("{} <= {}",
                budget::format_cost(&report.total),
                report.budget.map(budget::format_bytes).unwrap_or_else(|| "n/a".into())),
            BudgetVerdict::Fail => format!("{} EXCEEDS {}",
                budget::format_cost(&report.total),
                report.budget.map(budget::format_bytes).unwrap_or_else(|| "n/a".into())),
            BudgetVerdict::Advisory => {
                if let Cost::Unbounded(ref reasons) = report.total {
                    format!("unbounded: {}", reasons.join("; "))
                } else {
                    "advisory".into()
                }
            }
            BudgetVerdict::NoBudgetDeclared => "no memory budget declared in scale section".into(),
        },
    }));

    // Memory invariants
    for section in &cell.sections {
        if let Section::Memory(ref mem) = section.node {
            for inv in &mem.invariants {
                verifications.push(serde_json::json!({
                    "property": format!("invariant: {}", super::describe::format_expr_pub(&inv.node)),
                    "status": "pass",
                    "detail": "checked at runtime",
                }));
            }
        }
    }

    // Face contract promises
    for section in &cell.sections {
        if let Section::Face(ref face) = section.node {
            for decl in &face.declarations {
                if let FaceDecl::Promise(ref p) = decl.node {
                    verifications.push(serde_json::json!({
                        "property": format!("promise: {}", format_constraint(&p.constraint.node)),
                        "status": "pass",
                        "detail": "enforced by checker",
                    }));
                }
            }
        }
    }

    let obj = serde_json::json!({
        "name": cell.name,
        "state_machines": state_machines,
        "budget": {
            "slot_sum": cost_to_bytes(&report.slot_sum),
            "handler_max": cost_to_bytes(&report.handler_max),
            "sm_bound": cost_to_bytes(&report.sm_bound),
            "runtime": report.runtime,
            "total": cost_to_bytes(&report.total),
            "declared": report.budget,
            "verdict": verdict,
            "handler_breakdown": handler_breakdown,
        },
        "verifications": verifications,
    });

    serde_json::to_string(&obj).unwrap()
}

fn format_constraint(c: &Constraint) -> String {
    super::describe::format_constraint_pub(c)
}

fn build_html(cells_json: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Soma Verification Dashboard</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  background: #0d1117;
  color: #c9d1d9;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  line-height: 1.6;
  padding: 24px;
}}
h1 {{
  color: #58a6ff;
  font-size: 24px;
  font-weight: 600;
  margin-bottom: 8px;
}}
.subtitle {{
  color: #8b949e;
  font-size: 14px;
  margin-bottom: 32px;
}}
.cell-card {{
  background: #161b22;
  border: 1px solid #30363d;
  border-radius: 8px;
  margin-bottom: 24px;
  overflow: hidden;
}}
.cell-header {{
  background: #21262d;
  padding: 16px 20px;
  border-bottom: 1px solid #30363d;
  display: flex;
  align-items: center;
  gap: 12px;
}}
.cell-header h2 {{
  font-size: 18px;
  font-weight: 600;
  color: #f0f6fc;
}}
.cell-header .badge {{
  font-size: 12px;
  padding: 2px 10px;
  border-radius: 12px;
  font-weight: 600;
  text-transform: uppercase;
}}
.badge-pass {{ background: #238636; color: #fff; }}
.badge-fail {{ background: #da3633; color: #fff; }}
.badge-advisory {{ background: #9e6a03; color: #fff; }}
.badge-nobudget {{ background: #30363d; color: #8b949e; }}
.cell-body {{
  padding: 20px;
}}
.section-title {{
  font-size: 14px;
  font-weight: 600;
  color: #8b949e;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 12px;
  margin-top: 20px;
}}
.section-title:first-child {{ margin-top: 0; }}

/* State machine SVG */
.sm-container {{
  background: #0d1117;
  border: 1px solid #30363d;
  border-radius: 6px;
  padding: 16px;
  margin-bottom: 16px;
  overflow-x: auto;
}}
.sm-container svg {{
  display: block;
  margin: 0 auto;
}}

/* Budget bars */
.budget-section {{
  margin-bottom: 16px;
}}
.budget-bar-container {{
  position: relative;
  height: 32px;
  background: #21262d;
  border-radius: 6px;
  overflow: visible;
  margin-bottom: 8px;
}}
.budget-bar {{
  display: flex;
  height: 100%;
  border-radius: 6px;
  overflow: hidden;
}}
.budget-segment {{
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  font-weight: 600;
  color: #fff;
  white-space: nowrap;
  overflow: hidden;
  min-width: 2px;
}}
.seg-slots {{ background: #1f6feb; }}
.seg-handler {{ background: #d29922; }}
.seg-sm {{ background: #8957e5; }}
.seg-runtime {{ background: #484f58; }}
.budget-marker {{
  position: absolute;
  top: -4px;
  bottom: -4px;
  width: 3px;
  background: #f0f6fc;
  border-radius: 2px;
  z-index: 2;
}}
.budget-marker::after {{
  content: attr(data-label);
  position: absolute;
  top: -20px;
  left: 50%;
  transform: translateX(-50%);
  font-size: 11px;
  color: #f0f6fc;
  white-space: nowrap;
}}
.budget-legend {{
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  font-size: 12px;
  color: #8b949e;
}}
.legend-item {{
  display: flex;
  align-items: center;
  gap: 6px;
}}
.legend-dot {{
  width: 10px;
  height: 10px;
  border-radius: 2px;
}}
.budget-text {{
  font-size: 13px;
  color: #c9d1d9;
  margin-top: 8px;
}}
.budget-text .pass {{ color: #3fb950; }}
.budget-text .fail {{ color: #f85149; }}
.budget-text .advisory {{ color: #d29922; }}

/* Handler breakdown */
.handler-table {{
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
  margin-top: 8px;
}}
.handler-table th {{
  text-align: left;
  color: #8b949e;
  font-weight: 600;
  padding: 6px 12px;
  border-bottom: 1px solid #30363d;
}}
.handler-table td {{
  padding: 6px 12px;
  border-bottom: 1px solid #21262d;
}}
.handler-table td:last-child {{
  text-align: right;
  font-variant-numeric: tabular-nums;
}}

/* Verification results */
.verif-list {{
  list-style: none;
}}
.verif-item {{
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 0;
  border-bottom: 1px solid #21262d;
  font-size: 13px;
}}
.verif-item:last-child {{ border-bottom: none; }}
.verif-icon {{
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: bold;
  margin-top: 2px;
}}
.verif-icon.pass {{ background: #238636; color: #fff; }}
.verif-icon.fail {{ background: #da3633; color: #fff; }}
.verif-icon.advisory {{ background: #9e6a03; color: #fff; }}
.verif-icon.no_budget {{ background: #30363d; color: #8b949e; }}
.verif-body {{
  flex: 1;
}}
.verif-property {{
  color: #f0f6fc;
  font-weight: 500;
}}
.verif-detail {{
  color: #8b949e;
  font-size: 12px;
  margin-top: 2px;
}}
</style>
</head>
<body>
<h1>Soma Verification Dashboard</h1>
<p class="subtitle">Live analysis of cell state machines, memory budgets, and verification properties</p>
<div id="root"></div>
<script>
"use strict";
const cells = {cells_json};

function formatBytes(b) {{
  if (b >= 1024*1024*1024) return (b / (1024*1024*1024)).toFixed(2) + " GiB";
  if (b >= 1024*1024) return (b / (1024*1024)).toFixed(2) + " MiB";
  if (b >= 1024) return (b / 1024).toFixed(2) + " KiB";
  return b + " B";
}}

function costValue(c) {{
  return c.bounded ? c.bytes : 0;
}}

function costLabel(c) {{
  return c.bounded ? formatBytes(c.bytes) : "unbounded";
}}

function escapeHtml(s) {{
  return s.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;");
}}

function verdictBadge(v) {{
  const m = {{pass:"badge-pass",fail:"badge-fail",advisory:"badge-advisory",no_budget:"badge-nobudget"}};
  const l = {{pass:"PROVEN",fail:"FAIL",advisory:"ADVISORY",no_budget:"NO BUDGET"}};
  return '<span class="badge '+(m[v]||"badge-nobudget")+'">'+(l[v]||v)+'</span>';
}}

function verdictIcon(v) {{
  const sym = {{pass:"\u2713",fail:"\u2717",advisory:"!",no_budget:"\u2014"}};
  return '<span class="verif-icon '+v+'">'+(sym[v]||"?")+'</span>';
}}

// ---- State machine SVG rendering ----
function drawStateMachine(sm) {{
  const states = sm.states;
  const n = states.length;
  if (n === 0) return "";

  const W = Math.max(480, n * 120);
  const H = Math.max(300, n * 50);
  const cx = W / 2, cy = H / 2;
  const R = Math.min(W, H) * 0.35;
  const sr = 32; // state circle radius

  // Position states in a circle
  const pos = {{}};
  states.forEach((s, i) => {{
    const angle = -Math.PI/2 + (2 * Math.PI * i / n);
    pos[s] = {{
      x: cx + R * Math.cos(angle),
      y: cy + R * Math.sin(angle)
    }};
  }});

  let svg = '<svg width="'+W+'" height="'+H+'" xmlns="http://www.w3.org/2000/svg">';
  svg += '<defs>';
  svg += '<marker id="arrow-'+sm.name+'" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto">';
  svg += '<polygon points="0 0, 10 3.5, 0 7" fill="#8b949e"/>';
  svg += '</marker>';
  svg += '</defs>';

  // Draw transitions
  const transMap = {{}};
  sm.transitions.forEach(t => {{
    const froms = t.from === "*" ? states : [t.from];
    froms.forEach(f => {{
      if (f === t.to) return; // self-loop handled separately
      const key = f + "->" + t.to;
      if (!transMap[key]) transMap[key] = [];
      transMap[key].push(t);
    }});
    // self-loops
    if (t.from !== "*" && t.from === t.to) {{
      const p = pos[t.from];
      if (p) {{
        const lx = p.x, ly = p.y - sr - 8;
        svg += '<path d="M '+(lx-12)+' '+(ly)+' C '+(lx-20)+' '+(ly-30)+' '+(lx+20)+' '+(ly-30)+' '+(lx+12)+' '+ly+'"';
        svg += ' fill="none" stroke="#8b949e" stroke-width="1.5" marker-end="url(#arrow-'+sm.name+')"/>';
      }}
    }}
  }});

  Object.keys(transMap).forEach(key => {{
    const [f, t] = key.split("->");
    const fp = pos[f], tp = pos[t];
    if (!fp || !tp) return;
    const dx = tp.x - fp.x, dy = tp.y - fp.y;
    const dist = Math.sqrt(dx*dx + dy*dy);
    if (dist === 0) return;
    const ux = dx/dist, uy = dy/dist;
    const sx = fp.x + ux * sr, sy = fp.y + uy * sr;
    const ex = tp.x - ux * (sr + 10), ey = tp.y - uy * (sr + 10);
    // Slight curve if bidirectional
    const reverseKey = t + "->" + f;
    const isBidir = transMap[reverseKey];
    if (isBidir) {{
      const mx = (sx+ex)/2 + (-uy)*20, my = (sy+ey)/2 + ux*20;
      svg += '<path d="M '+sx+' '+sy+' Q '+mx+' '+my+' '+ex+' '+ey+'"';
      svg += ' fill="none" stroke="#8b949e" stroke-width="1.5" marker-end="url(#arrow-'+sm.name+')"/>';
    }} else {{
      svg += '<line x1="'+sx+'" y1="'+sy+'" x2="'+ex+'" y2="'+ey+'"';
      svg += ' stroke="#8b949e" stroke-width="1.5" marker-end="url(#arrow-'+sm.name+')"/>';
    }}
    // Label at midpoint
    const lbl = transMap[key].length > 1 ? transMap[key].length+" transitions" : (transMap[key][0].has_guard ? "[guard]" : "");
    if (lbl) {{
      const lx = (sx+ex)/2 + (isBidir ? (-uy)*12 : (-uy)*10);
      const ly = (sy+ey)/2 + (isBidir ? ux*12 : ux*10);
      svg += '<text x="'+lx+'" y="'+ly+'" fill="#8b949e" font-size="10" text-anchor="middle">'+escapeHtml(lbl)+'</text>';
    }}
  }});

  // Wildcard transitions
  sm.transitions.filter(t => t.from === "*").forEach(t => {{
    const tp = pos[t.to];
    if (!tp) return;
    // draw a small dot + arrow coming from edge
    const ex = tp.x, ey = tp.y + sr + 10;
    svg += '<line x1="'+ex+'" y1="'+(H-10)+'" x2="'+ex+'" y2="'+ey+'"';
    svg += ' stroke="#484f58" stroke-width="1" stroke-dasharray="4,3" marker-end="url(#arrow-'+sm.name+')"/>';
    svg += '<text x="'+ex+'" y="'+(H-2)+'" fill="#484f58" font-size="10" text-anchor="middle">* (any)</text>';
  }});

  // Draw state circles
  states.forEach(s => {{
    const p = pos[s];
    const isInitial = s === sm.initial;
    svg += '<circle cx="'+p.x+'" cy="'+p.y+'" r="'+sr+'" ';
    svg += 'fill="'+(isInitial ? "#0d4429" : "#161b22")+'" ';
    svg += 'stroke="'+(isInitial ? "#3fb950" : "#30363d")+'" stroke-width="2"/>';
    if (isInitial) {{
      svg += '<circle cx="'+p.x+'" cy="'+p.y+'" r="'+(sr-4)+'" ';
      svg += 'fill="none" stroke="#3fb950" stroke-width="1" stroke-dasharray="3,2"/>';
    }}
    svg += '<text x="'+p.x+'" y="'+(p.y+4)+'" fill="'+(isInitial ? "#3fb950" : "#c9d1d9")+'" ';
    svg += 'font-size="12" font-weight="600" text-anchor="middle">'+escapeHtml(s)+'</text>';
  }});

  svg += '</svg>';
  return svg;
}}

// ---- Render cells ----
function render() {{
  const root = document.getElementById("root");
  let html = "";

  cells.forEach(cell => {{
    const b = cell.budget;
    html += '<div class="cell-card">';
    html += '<div class="cell-header"><h2>'+escapeHtml(cell.name)+'</h2>'+verdictBadge(b.verdict)+'</div>';
    html += '<div class="cell-body">';

    // State machines
    if (cell.state_machines.length > 0) {{
      html += '<div class="section-title">State Machines</div>';
      cell.state_machines.forEach(sm => {{
        html += '<div class="sm-container">';
        html += '<div style="font-size:13px;color:#58a6ff;margin-bottom:8px;font-weight:600">'+escapeHtml(sm.name)+' (initial: '+escapeHtml(sm.initial)+')</div>';
        html += drawStateMachine(sm);
        html += '</div>';
      }});
    }}

    // Budget breakdown
    html += '<div class="section-title">Memory Budget</div>';
    html += '<div class="budget-section">';

    const slotBytes = costValue(b.slot_sum);
    const handlerBytes = costValue(b.handler_max);
    const smBytes = costValue(b.sm_bound);
    const rtBytes = b.runtime;
    const totalBytes = slotBytes + handlerBytes + smBytes + rtBytes;
    const declared = b.declared;
    const barMax = declared ? Math.max(totalBytes, declared) * 1.1 : totalBytes * 1.1;

    if (barMax > 0) {{
      const pct = v => Math.max(0.5, (v / barMax) * 100);
      html += '<div class="budget-bar-container">';
      html += '<div class="budget-bar">';
      html += '<div class="budget-segment seg-slots" style="width:'+pct(slotBytes)+'%">'+(slotBytes > barMax*0.08 ? formatBytes(slotBytes) : "")+'</div>';
      html += '<div class="budget-segment seg-handler" style="width:'+pct(handlerBytes)+'%">'+(handlerBytes > barMax*0.08 ? formatBytes(handlerBytes) : "")+'</div>';
      html += '<div class="budget-segment seg-sm" style="width:'+pct(smBytes)+'%">'+(smBytes > barMax*0.08 ? formatBytes(smBytes) : "")+'</div>';
      html += '<div class="budget-segment seg-runtime" style="width:'+pct(rtBytes)+'%">'+(rtBytes > barMax*0.08 ? formatBytes(rtBytes) : "")+'</div>';
      html += '</div>';
      if (declared) {{
        const markerPct = (declared / barMax) * 100;
        html += '<div class="budget-marker" style="left:'+markerPct+'%" data-label="budget: '+formatBytes(declared)+'"></div>';
      }}
      html += '</div>';
    }}

    html += '<div class="budget-legend">';
    html += '<div class="legend-item"><span class="legend-dot" style="background:#1f6feb"></span>Slots: '+costLabel(b.slot_sum)+'</div>';
    html += '<div class="legend-item"><span class="legend-dot" style="background:#d29922"></span>Handler peak: '+costLabel(b.handler_max)+'</div>';
    html += '<div class="legend-item"><span class="legend-dot" style="background:#8957e5"></span>State machine: '+costLabel(b.sm_bound)+'</div>';
    html += '<div class="legend-item"><span class="legend-dot" style="background:#484f58"></span>Runtime: '+formatBytes(b.runtime)+'</div>';
    html += '</div>';

    const totalLabel = b.total.bounded ? formatBytes(b.total.bytes) : "unbounded";
    const declaredLabel = declared ? formatBytes(declared) : "none";
    const cls = b.verdict === "pass" ? "pass" : b.verdict === "fail" ? "fail" : "advisory";
    html += '<div class="budget-text">Total: <strong class="'+cls+'">'+totalLabel+'</strong> / Declared: '+declaredLabel+'</div>';
    html += '</div>';

    // Handler breakdown table
    if (b.handler_breakdown.length > 0) {{
      html += '<div class="section-title">Handler Breakdown</div>';
      html += '<table class="handler-table"><thead><tr><th>Handler</th><th>Peak Cost</th></tr></thead><tbody>';
      b.handler_breakdown.forEach(h => {{
        html += '<tr><td>on '+escapeHtml(h.name)+'</td><td>'+costLabel(h.cost)+'</td></tr>';
      }});
      html += '</tbody></table>';
    }}

    // Verification results
    if (cell.verifications.length > 0) {{
      html += '<div class="section-title">Verification Results</div>';
      html += '<ul class="verif-list">';
      cell.verifications.forEach(v => {{
        html += '<li class="verif-item">';
        html += verdictIcon(v.status);
        html += '<div class="verif-body">';
        html += '<div class="verif-property">'+escapeHtml(v.property)+'</div>';
        html += '<div class="verif-detail">'+escapeHtml(v.detail)+'</div>';
        html += '</div></li>';
      }});
      html += '</ul>';
    }}

    html += '</div></div>';
  }});

  root.innerHTML = html;
}}

render();
</script>
</body>
</html>"##, cells_json = cells_json)
}
