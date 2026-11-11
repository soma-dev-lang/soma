//! 3.0: the run-time side of a tool bound to a server (see `external.rs`).

use super::*;

/// Another server's JSON never becomes one of OUR records (as for http_get).
fn reserved(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(o) => o.keys().any(|k| matches!(k.as_str(), "_type" | "_variant" | "_values" | "_status" | "_response")) || o.values().any(reserved),
        serde_json::Value::Array(a) => a.iter().any(reserved),
        _ => false,
    }
}

impl Interpreter {
    /// `_external_tool("name", args…)`: run the bound tool `name` of this cell.
    pub(super) fn call_external_tool(&mut self, cell_name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let Some(Value::String(tool_name)) = args.first().cloned() else {
            return Err(RuntimeError::TypeError("_external_tool is the body of a bound tool; it is not called by name".to_string()));
        };
        let tool = self.cells.get(cell_name).and_then(|cell| cell.sections.iter().find_map(|s| match &s.node {
            Section::Face(face) => face.declarations.iter().find_map(|d| match &d.node {
                FaceDecl::Tool(t) if t.name == tool_name && t.binding.is_some() => Some(t.clone()),
                _ => None,
            }),
            _ => None,
        }));
        let Some(tool) = tool else {
            return Err(RuntimeError::TypeError(format!("'{}' is not a tool of cell '{}' bound to a server", tool_name, cell_name)));
        };
        let fail = |kind: &str, message: String| RuntimeError::Domain { kind: kind.to_string(), message };
        // a test never reaches the network: the answer is scripted
        if IN_TEST.load(std::sync::atomic::Ordering::Relaxed) || self.test_auto_mock {
            return Err(fail("tool", format!(
                "tool: '{}' runs on the server `{}` — a test does not call it: script its answer with `mock {} …` before the call",
                tool_name, tool.binding.as_ref().map(|b| b.server.as_str()).unwrap_or(""), tool_name)));
        }
        let mut object = serde_json::Map::new();
        for (param, value) in tool.params.iter().zip(args.into_iter().skip(1)) {
            if matches!(value, Value::Unit) { continue; }
            object.insert(param.name.clone(), record_log::value_to_json(&value));
        }
        // the tool's own capabilities, and those of the tool call in progress
        let own = tool.capabilities.clone();
        let current = self.current_tool_caps.clone();
        let outer = self.outer_tool_caps.clone();
        let allow = move |url: &str| -> Result<(), String> {
            let scopes = (!own.is_empty()).then_some(&own).into_iter().chain(current.iter()).chain(outer.iter());
            for caps in scopes {
                if !builtins::url_matches_any(url, caps) {
                    return Err(format!("capability denied: '{}' does not match any of {:?}", builtins::http::redact_url(url), caps));
                }
            }
            Ok(())
        };
        // the request waits outside the handler lock in a `[task]`, as a model call does
        let outcome = self.outside_unit(|| crate::external::call(&tool, object, &allow))?;
        let json = outcome.map_err(|e| fail(e.kind, if e.message.starts_with(e.kind) { e.message } else { format!("{}: {}", e.kind, e.message) }))?;
        let declared = match tool.return_type.as_ref().map(|t| &t.node) {
            Some(TypeExpr::Simple(n)) => n.as_str(),
            Some(TypeExpr::Generic { name, .. }) => name.as_str(),
            _ => "Any",
        };
        // text that is JSON, for a tool declared to give a Map or a List
        let json = match (&json, declared) {
            (serde_json::Value::String(s), "Map" | "List") => serde_json::from_str::<serde_json::Value>(s.trim()).unwrap_or(json),
            _ => json,
        };
        let value = match (&json, declared) {
            (serde_json::Value::String(s), _) => Value::String(s.clone()),
            (other, "String") => Value::String(other.to_string()),
            (other, _) if reserved(other) => Value::String(other.to_string()),
            (other, _) => builtins::serde_json_to_value(other),
        };
        let fits = match (declared, &value) {
            ("Any", _) | ("String", Value::String(_)) | ("Int", Value::Int(_)) | ("Float", Value::Float(_) | Value::Int(_))
            | ("Bool", Value::Bool(_)) | ("Map", Value::Map(_)) | ("List", Value::List(_)) => true,
            ("Map" | "List" | "Int" | "Float" | "Bool" | "String", _) => false,
            _ => true,
        };
        if !fits {
            return Err(fail("tool", format!("tool: '{}' is declared `-> {}` but the server answered {}", tool_name, declared, value_type_name(&value))));
        }
        Ok(value)
    }
}
