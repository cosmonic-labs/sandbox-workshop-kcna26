//! model-real: the same `model` interface, backed by an OpenAI-compatible
//! chat completions endpoint.
//!
//! Configuration (environment, set in the manifest):
//!   LLM_URL      full URL of /v1/chat/completions
//!   LLM_MODEL    model name
//!   LLM_API_KEY  bearer token (optional; use a secret in real deployments)
//!
//! The endpoint's host must be in this component's allowedHosts. Nothing
//! else in the workload gets that host.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/model-provider",
    generate_all,
});

use exports::workshop::toolbox::model::Guest;
use serde_json::{Value, json};
use workshop::toolbox::types::{Completion, Message, ToolCall, ToolDef};
use wstd::http::{Body, Client, Request};

struct Real;

impl Guest for Real {
    fn complete(messages: Vec<Message>, tools: Vec<ToolDef>) -> Result<Completion, String> {
        let url = std::env::var("LLM_URL").map_err(|_| "LLM_URL is not set".to_string())?;
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        let key = std::env::var("LLM_API_KEY").ok();

        let mut wire = vec![json!({
            "role": "system",
            "content": "You are a helpful agent. Use the tools when they help."
        })];
        let mut call_n = 0;
        for m in &messages {
            match m.role.as_str() {
                "assistant" => {
                    call_n += 1;
                    let c: Value = serde_json::from_str(&m.content).unwrap_or(Value::Null);
                    wire.push(json!({ "role": "assistant", "content": null, "tool_calls": [{
                        "id": format!("call_{call_n}"), "type": "function",
                        "function": { "name": c["name"], "arguments": c["arguments"].to_string() }
                    }]}));
                }
                "tool" => wire.push(json!({
                    "role": "tool", "tool_call_id": format!("call_{call_n}"), "content": m.content
                })),
                _ => wire.push(json!({ "role": m.role, "content": m.content })),
            }
        }
        let tools: Vec<Value> = tools
            .iter()
            .map(|t| json!({ "type": "function", "function": {
                "name": t.name, "description": t.description,
                "parameters": serde_json::from_str::<Value>(&t.parameters).unwrap_or(json!({}))
            }}))
            .collect();
        let mut body = json!({ "model": model, "messages": wire });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }

        let resp: Value = wstd::runtime::block_on(async move {
            let mut req = Request::post(&url).header("content-type", "application/json");
            if let Some(k) = key {
                req = req.header("authorization", format!("Bearer {k}"));
            }
            let req = req
                .body(Body::from(body.to_string()))
                .map_err(|e| e.to_string())?;
            let resp = Client::new()
                .send(req)
                .await
                .map_err(|e| format!("POST {url} failed: {e:?}"))?;
            let bytes = resp.into_body().contents().await.map_err(|e| e.to_string())?.to_vec();
            serde_json::from_slice(&bytes).map_err(|e| format!("bad JSON from model: {e}"))
        })?;

        let msg = &resp["choices"][0]["message"];
        if let Some(call) = msg["tool_calls"].get(0) {
            return Ok(Completion::Call(ToolCall {
                name: call["function"]["name"].as_str().unwrap_or_default().into(),
                arguments: call["function"]["arguments"].as_str().unwrap_or("{}").into(),
            }));
        }
        match msg["content"].as_str() {
            Some(text) => Ok(Completion::Text(text.into())),
            None => Err(format!("unexpected model response: {resp}")),
        }
    }
}

export!(Real);
