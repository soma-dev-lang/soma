//! `soma docs` — render generated reference documentation.
//! The compiler is the single source of truth: this command renders the
//! builtin registry as markdown (committed as SOMA_BUILTINS.md).

use crate::interpreter::builtins::registry;

/// Hand-written references, embedded at build time: an agent without
/// network access still has the language, and the text cannot be newer or
/// older than the binary that prints it.
const AGENT_SUMMARY: &str = include_str!("../../../site/llms.txt");
const AGENTS_GUIDE: &str = include_str!("../../../site/llms-agents.txt");
const NATIVE_GUIDE: &str = include_str!("../../../site/llms-native.txt");
const SKELETON_GUIDE: &str = include_str!("../../../site/llms-skeleton.txt");
const METHOD: &str = include_str!("../../../site/llms-method.txt");
const MANIFESTO: &str = include_str!("../../../docs/site/manifesto.md");
const LAW_GUIDE: &str = include_str!("../../../site/llms-law.txt");
const FLESH_CARD: &str = include_str!("../../../site/llms-flesh.txt");
const LAW_BRIEF: &str = include_str!("../../../site/llms-law-brief.txt");
const SKELETON_BRIEF: &str = include_str!("../../../site/llms-skeleton-brief.txt");
const RELEASES: &str = include_str!("../../../site/llms-releases.txt");
const REFERENCE: &str = include_str!("../../../SOMA_REFERENCE.md");
const GOTCHAS: &str = include_str!("../../../AGENT_GOTCHAS.md");
const AGENTS_MD: &str = super::agent_setup::AGENTS_MD;
const GUARANTEES: &str = include_str!("../../../docs/site/guarantees.md");
const SERVING: &str = include_str!("../../../docs/site/serving.md");
const CLUSTER: &str = include_str!("../../../docs/site/cluster.md");
const ROBOTICS: &str = include_str!("../../../docs/robotics/ROBUSTNESS.md");
const OPERATIONS: &str = include_str!("../../../docs/site/operations.md");

const TOPICS: &[(&str, &str)] = &[
    ("agent", "working summary of the language for a model's context (start here)"),
    ("agents", "running LLM agents: tools, capabilities, [task], hordes, budgets, mocks, replay"),
    ("native", "the [native] vocabulary, its limits and costs"),
    ("manifesto", "The Economy of Trust: what Soma is for, and what it holds of it today"),
    ("method", "how a program is built with an agent: law, skeleton, flesh; the order of work; what each step establishes"),
    ("skeleton", "large programs: skeleton / flesh, frames, holes, the brief, skeleton.lock"),
    ("law", "business rules outside the code: the .law file, realizes / under / exempt, soma law"),
    ("flesh", "writing ONE body from its brief: the syntax a handler needs, on one page"),
    ("law --brief", "read BEFORE writing a law: what a good one contains, on one screen"),
    ("skeleton --brief", "read BEFORE writing a skeleton: what a good one contains, on one screen"),
    ("releases", "what each release changed, newest first"),
    ("reference", "the full language reference"),
    ("gotchas", "mistakes models make, each with the real diagnostic and the fix"),
    ("builtins", "every builtin with its exact signature, generated from the compiler"),
    ("agents-md", "drop-in AGENTS.md / CLAUDE.md block for a project that uses Soma"),
    ("guarantees", "what is PROVEN, ENFORCED AT RUNTIME, or NOT COVERED — by command"),
    ("serving", "soma serve: routing, exposure, body shape, error statuses, bind address"),
    ("operations", "what kills the process and what does not, ports, exit codes, limits, Linux deployment"),
    ("cluster", "experimental replication, recovery, upgrades and distributed guarantees"),
    ("robotics", "100 robotics robustness scenarios, reproduction and coverage limits"),
    ("all", "agent + agents + native + releases + robotics + cluster + reference + gotchas + guarantees + serving + operations + builtins, one stream"),
];

pub fn cmd_docs(topic: &str) {
    match topic {
        "builtins" => print_builtins_markdown(),
        "agent" | "llms" => print!("{}", AGENT_SUMMARY),
        "agents" => print!("{}", AGENTS_GUIDE),
        "native" | "performance" => print!("{}", NATIVE_GUIDE),
        "manifesto" => print!("{}", MANIFESTO),
        "method" => print!("{}", METHOD),
        "skeleton" | "frames" => print!("{}", SKELETON_GUIDE),
        "law" | "rules" => print!("{}", LAW_GUIDE),
        "flesh" | "body" | "flesh-brief" => print!("{}", FLESH_CARD),
        "law-brief" => print!("{}", LAW_BRIEF),
        "skeleton-brief" => print!("{}", SKELETON_BRIEF),
        "releases" => print!("{}", RELEASES),
        "reference" => print!("{}", REFERENCE),
        "gotchas" => print!("{}", GOTCHAS),
        "agents-md" => print!("{}", AGENTS_MD),
        "guarantees" => print!("{}", GUARANTEES),
        "serving" | "serve" | "http" => print!("{}", SERVING),
        "operations" | "ops" => print!("{}", OPERATIONS),
        "cluster" => print!("{}", CLUSTER),
        "robotics" => print!("{}", ROBOTICS),
        "all" => {
            print!("{}\n\n---\n\n", ROBOTICS);
            print!("{}\n\n---\n\n", CLUSTER);
            print!("{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n", AGENT_SUMMARY, AGENTS_GUIDE, NATIVE_GUIDE, format!("{}\n\n---\n\n{}", SKELETON_GUIDE, FLESH_CARD), RELEASES);
            print!("{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n", REFERENCE, GOTCHAS, GUARANTEES, SERVING, OPERATIONS);
            print_builtins_markdown();
        }
        "" | "topics" => {
            println!("soma docs <topic>\n");
            for (name, what) in TOPICS {
                println!("  {:<10} {}", name, what);
            }
            println!("\nOnline: https://soma-lang.dev/llms-full.txt");
        }
        other => {
            let names: Vec<&str> = TOPICS.iter().map(|(n, _)| *n).collect();
            eprintln!("error: unknown docs topic '{}'. Valid topics: [{}]", other, names.join(", "));
            std::process::exit(1);
        }
    }
}

/// Escape `|` so signatures survive inside a markdown table cell.
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

fn print_builtins_markdown() {
    // "reserved" names are tracked for replay but NOT callable — they used
    // to be listed here and an agent wrote `timestamp()` from this table
    let listed: Vec<&registry::BuiltinDoc> = registry::BUILTINS.iter().filter(|b| b.category != "reserved").collect();
    let total = listed.len();
    let nondet: Vec<&str> = listed.iter()
        .filter(|b| !b.deterministic)
        .map(|b| b.name)
        .collect();

    println!("# Soma Builtins");
    println!();
    println!("> GENERATED by `soma docs builtins` — do not edit by hand.");
    println!("> The source of truth is `compiler/src/interpreter/builtins/registry.rs`.");
    println!();
    println!("{} builtins. ✗ marks the nondeterministic set ({}) — calls to these", total, nondet.join(", "));
    println!("are tracked by `soma replay` as potential sources of replay divergence.");
    println!("`deterministic` is membership in that replay set, not a purity claim:");
    println!("think/http_*/read_*/next_id are NOT in the log: replay calls them again (think needs a key or");
    println!("SOMA_LLM_MOCK, files are re-read) — pass their results in as handler arguments to replay exactly.");
    println!("The `native` section is usable inside `[native]` handlers only.");

    for cat in registry::categories() {
        if cat == "reserved" { continue; }
        println!();
        println!("## {}", cat);
        println!();
        println!("| Builtin | Signature | Description |");
        println!("|---|---|---|");
        for b in listed.iter().filter(|b| b.category == cat) {
            let marker = if b.deterministic { "" } else { " ✗" };
            println!("| `{}`{} | `{}` | {} |",
                b.name, marker, md_cell(b.signature), md_cell(b.brief));
        }
    }
}
