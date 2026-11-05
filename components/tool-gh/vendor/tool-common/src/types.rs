// Vendored from cosmonic-labs/agent-tools (crates/tool-common/src/types.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Unmodified.

//! Tool definitions, replies and errors, in the shape of
//! `cosmonic:agent/tool-types@0.4.0` but free of bindings, so the tool logic
//! is tested natively and `toolset-glue` converts at the edge.

/// One tool as offered to a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDef {
    /// The model's selection key.
    pub name: &'static str,
    /// Display label.
    pub title: &'static str,
    pub description: String,
    /// JSON Schema for the arguments object.
    pub input_schema: String,
    /// JSON Schema for the result's structured JSON, when the tool
    /// promises one.
    pub output_schema: Option<String>,
    /// Display hints; the host's grants are what enforce anything.
    pub read_only: bool,
    pub idempotent: bool,
    /// Whether the tool reaches beyond the machine (a web API).
    pub open_world: bool,
}

/// A completed call: what the model sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Text for the model, already within [`crate::bound::TEXT_MAX`].
    pub text: String,
    /// Machine-readable JSON, within [`crate::bound::STRUCTURED_MAX`].
    pub structured: Option<String>,
    /// The tool ran and failed; the model reads `text` as a normal result.
    pub is_error: bool,
}

impl Reply {
    pub fn ok(text: String, structured: Option<String>) -> Self {
        Self {
            text,
            structured,
            is_error: false,
        }
    }

    /// A completed failure: the tool ran, had no effects, and says why.
    pub fn failed(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: true,
        }
    }
}

/// Why a call did not start. Nothing happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    UnknownTool(String),
    InvalidArguments(String),
    /// The upstream refused or could not be reached before the call started
    /// (for the GitHub tool: not sent, or refused by a rate limit).
    Upstream(String),
    Denied(String),
    Cancelled,
    TimedOut,
    Other(String),
}

/// No terminal response; effects may have occurred. The tools here are read
/// only, so "effects" can only mean a request that reached its server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Upstream(String),
    Cancelled,
    TimedOut,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    NotDispatched(Dispatch),
    OutcomeUnknown(Outcome),
}

impl CallError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::NotDispatched(Dispatch::InvalidArguments(msg.into()))
    }

    pub fn unknown_tool(name: &str, known: &[&str]) -> Self {
        Self::NotDispatched(Dispatch::UnknownTool(format!(
            "no tool named {name:?}; this toolset offers {}",
            known.join(", ")
        )))
    }
}
