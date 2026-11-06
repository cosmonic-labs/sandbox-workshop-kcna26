//! The tools the model sees: whatever `tools.list` returns.
//!
//! The harness holds no tool table of its own. It imports the agent SDK's
//! `tools` interface (cosmonic:agent), which the toolbox host plugin serves:
//! the plugin lists the tools this workload's manifest gives the agent, and
//! calls the tool component in this same workload for each call the model
//! makes. The plugin also does the translating a host could do from the WIT
//! (typed calls in, typed errors out as `{"case": "payload"}`); see
//! components/toolbox-plugin.

use crate::cosmonic::agent::inference_types::{ToolArguments, ToolCall, ToolInput, ToolOutput};
use crate::cosmonic::agent::tool_types::{CallError, DispatchError, OutcomeError, ToolDef};
use crate::cosmonic::agent::tools;
use serde_json::Value;

/// The tools in this workload. Empty when the manifest gives the agent none,
/// or when the plugin can't list them.
pub async fn list() -> Vec<ToolDef> {
    tools::list().await.unwrap_or_default()
}

/// The JSON Schema the model sees for a tool's arguments.
pub fn schema(t: &ToolDef) -> String {
    match &t.input {
        ToolInput::JsonSchema(s) => s.clone(),
        _ => r#"{"type":"object"}"#.into(),
    }
}

/// Make one call: the model's JSON arguments in, text out. A failure comes
/// back as `{"case": "payload"}`. A tool the workload doesn't have still goes
/// to the plugin, which refuses it.
pub async fn call(id: usize, name: &str, args: &Value) -> Result<String, String> {
    let request =
        ToolCall { id: format!("call-{id}"), name: name.into(), arguments: ToolArguments::Json(args.to_string()) };
    match tools::call(request).await {
        Ok(resp) => {
            let text: String = resp
                .output
                .iter()
                .filter_map(|o| match o {
                    ToolOutput::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if resp.is_error { Err(text) } else { Ok(text) }
        }
        Err(CallError::NotDispatched(e)) => Err(match e {
            DispatchError::UnknownTool(m) => variant("not-found", Some(m)),
            DispatchError::InvalidArguments(m) => variant("invalid-argument", Some(m)),
            DispatchError::Denied(m) => variant("denied", Some(m)),
            DispatchError::Upstream(m) | DispatchError::Other(m) => variant("other", Some(m)),
            DispatchError::Cancelled | DispatchError::TimedOut => variant("other", None),
        }),
        Err(CallError::OutcomeUnknown(e)) => Err(variant(
            "other",
            match e {
                OutcomeError::Upstream(m) | OutcomeError::Other(m) => Some(m),
                _ => None,
            },
        )),
    }
}

/// A WIT variant case as JSON: `{"case": payload}`.
fn variant(case: &str, payload: Option<String>) -> String {
    let mut map = serde_json::Map::new();
    map.insert(case.to_string(), payload.map_or(Value::Null, Value::String));
    Value::Object(map).to_string()
}
