// Vendored from cosmonic-labs/agent-tools (crates/tool-common/src/lib.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Modified: the workspace module (folder rules) is left out.

//! What every tool component in this repository shares.
//!
//! Nothing here touches WASI, so all of it is tested natively:
//!
//! - [`types`]: tool definitions, replies and the call error split
//!   (not dispatched versus outcome unknown), independent of the
//!   `cosmonic:agent/tools` version a component serves them on;
//! - [`args`]: strict parsing of a model's JSON arguments;
//! - [`bound`]: the output budget every reply is held to;
//! - [`http`]: outbound GET as the networked tools see it, and what a
//!   request with no response means for the call;

pub mod args;
pub mod bound;
pub mod http;
pub mod types;

pub use types::{CallError, Dispatch, Outcome, Reply, ToolDef};
