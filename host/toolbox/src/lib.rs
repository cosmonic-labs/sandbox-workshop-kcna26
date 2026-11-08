//! toolbox: a native wasmCloud host plugin that serves the agent SDK's
//! `tools` interface (`cosmonic:agent/tools`) to a workload by calling the
//! tool components in that same workload. It is the host side of tool
//! discovery: an agent imports `tools` and gets whatever tools its manifest
//! gives it. Cosmonic Desktop's dispatcher works the same way, built into its
//! host.
//!
//! It is compiled into the workshop's host (scripts/build-host.sh), so every
//! host in the cluster has it. A workload reaches it by declaring
//! `cosmonic:agent/tools` under `hostInterfaces`, and offers it tools in two
//! ways:
//!
//! - A typed tool (`curl`, `sqlite`, `gh`): declare the tool's interface under
//!   `hostInterfaces` and run a component that exports it.
//! - A toolset that speaks `cosmonic:agent/tools` itself, such as an MCP
//!   server composed with Cosmonic's MCP bridge: run a component that exports
//!   it under the plain name `cosmonic-toolset`, as Desktop does. Nothing
//!   imports that name, so the host links nothing to it, and the agent's own
//!   `tools` import can only come here.
//!
//! The plugin calls each tool on that tool's own instance, in the caller's own
//! workload, with that tool's own grant. It has no network and no folders: it
//! adds no reach of its own, and a workload can never call another workload's
//! tools through it.
//!
//! It is also the translation table. A model only understands JSON. It is
//! shown a list of tools, each with a name, a description and a JSON Schema
//! for its arguments, and it answers with a name and a JSON object. The typed
//! tools only understand typed functions such as `curl.run(run-args)`.
//! Desktop and Control translate between the two for any typed tool, reading
//! the tool's WIT. This plugin knows three tools, so it is written out by
//! hand, one row per function, to match `wit/toolbox.wit`:
//!
//! - Each function takes one `<name>-args` record with one field, the tool's
//!   argument. The field's doc comment is what the model is told to put there.
//! - The function's doc comment is the tool's description.
//! - The tool's name reads better to a model than the function's (`curl` for
//!   the curl tool's `run`).
//! - A typed error is a completed call that failed: a `tool-response` with
//!   `is-error` set, its text `{"case": "payload"}`, the JSON form of a WIT
//!   variant, so the model can tell a denied request from a network failure.
//!   A tool this workload doesn't have was never dispatched:
//!   `not-dispatched(unknown-tool)`.
//!
//! A toolset's tools need no table: it lists them itself.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use wash_runtime::engine::ctx::{ActiveCtx, SharedCtx, extract_active_ctx};
use wash_runtime::engine::dispatch::{DispatchTarget, GuestCall, GuestCallFuture};
use wash_runtime::engine::workload::{ResolvedWorkload, WorkloadItem};
use wash_runtime::plugin::{HostPlugin, WitInterfaces};
use wasmtime::component::{Accessor, Instance, Val};
use wash_runtime::wit::{WitInterface, WitWorld};

mod bindings {
    wasmtime::component::bindgen!({
        world: "workshop:toolbox-host/toolbox-host",
        path: "wit",
        imports: { default: store | async | trappable },
    });
}

use bindings::cosmonic::agent::inference_types::{ToolArguments, ToolCall, ToolInput, ToolOutput};
use bindings::cosmonic::agent::tool_types::{
    CallError, DispatchError, ListError, OutcomeError, ToolAnnotations, ToolDef, ToolResponse,
};
use bindings::cosmonic::agent::tools;

pub const PLUGIN_ID: &str = "workshop-toolbox";

/// The plain name a toolset exports `cosmonic:agent/tools` under, as on
/// Cosmonic Desktop. Nothing imports it.
pub const TOOLSET_EXPORT: &str = "cosmonic-toolset";

/// One function a model may call.
struct Row {
    /// The tool's interface in `workshop:toolbox`.
    interface: &'static str,
    /// The function the row calls.
    func: &'static str,
    /// The name the model uses.
    name: &'static str,
    /// The function's doc comment from the WIT.
    description: &'static str,
    /// The `<name>-args` record's one field, and its doc comment.
    argument: (&'static str, &'static str),
    /// (read-only, open-world): what the tool claims about itself, for display.
    hints: (bool, bool),
}

/// The whole table.
const TABLE: &[Row] = &[
    Row {
        interface: "curl",
        func: "run",
        name: "curl",
        description: "Make an HTTP request with curl and return what it prints. Only the \
                      hosts this tool is granted can be reached.",
        argument: ("arguments", "What you would type after curl, such as -s http://api.mock-sites.svc/data."),
        hints: (false, true),
    },
    Row {
        interface: "sqlite",
        func: "query",
        name: "sqlite",
        description: "Run SQL against this tool's database and return the rows. Also accepts \
                      dot-commands such as .tables and .schema.",
        argument: ("sql", "SQL to run, such as select title from talks limit 5, or a dot-command such as .tables."),
        hints: (true, false),
    },
    Row {
        interface: "gh",
        func: "run",
        name: "gh",
        description: "Read pull requests on GitHub with gh and return what it prints: pr list, \
                      pr view, pr checks, pr diff, search prs. Only reads.",
        argument: ("arguments", "What you would type after gh, such as pr list -R wasmCloud/wasmCloud."),
        hints: (true, true),
    },
];

impl Row {
    fn def(&self) -> ToolDef {
        let (field, doc) = self.argument;
        ToolDef {
            name: self.name.into(),
            title: None,
            description: self.description.into(),
            input: ToolInput::JsonSchema(
                json!({
                    "type": "object",
                    "properties": { field: { "type": "string", "description": doc } },
                    "required": [field],
                })
                .to_string(),
            ),
            output_schema: None,
            annotations: ToolAnnotations {
                read_only: Some(self.hints.0),
                destructive: Some(false),
                idempotent: None,
                open_world: Some(self.hints.1),
            },
            version: None,
        }
    }
}

/// A typed tool this workload offers: its row, the full name of the
/// interface its component exports, and where to call it.
struct Typed {
    row: &'static Row,
    export: String,
    target: DispatchTarget,
}

/// What one workload offers.
#[derive(Default)]
struct Offer {
    typed: Vec<Arc<Typed>>,
    toolsets: Vec<Arc<DispatchTarget>>,
    /// Which toolset answered to each name at the last list.
    routes: HashMap<String, usize>,
}

#[derive(Default)]
pub struct Toolbox {
    offers: Mutex<HashMap<String, Offer>>,
}

/// One call on a tool component's instance.
enum Job {
    Typed {
        export: String,
        func: &'static str,
        field: &'static str,
        argument: String,
        reply: oneshot::Sender<Result<String, String>>,
    },
    List {
        reply: oneshot::Sender<Result<Vec<ToolDef>, ListError>>,
    },
    Call {
        request: ToolCall,
        reply: oneshot::Sender<Result<ToolResponse, CallError>>,
    },
}

fn wt(e: wasmtime::Error) -> anyhow::Error {
    anyhow::anyhow!("{e:#}")
}

/// The index of `func` in the instance `export` the component exports.
fn export_func(
    accessor: &Accessor<SharedCtx>,
    instance: Instance,
    export: &str,
    func: &str,
) -> anyhow::Result<wasmtime::component::ComponentExportIndex> {
    accessor.with(|access| {
        let pre = instance.instance_pre(&access);
        let c = pre.component();
        let iface = c
            .get_export_index(None, export)
            .with_context(|| format!("no exported instance named {export}"))?;
        c.get_export_index(Some(&iface), func)
            .with_context(|| format!("{export} has no {func}"))
    })
}

impl GuestCall for Job {
    fn describe(&self) -> &str {
        match self {
            Job::Typed { .. } => "workshop:toolbox#tool",
            Job::List { .. } => "cosmonic-toolset#list",
            Job::Call { .. } => "cosmonic-toolset#call",
        }
    }

    fn call<'a>(self: Box<Self>, accessor: &'a Accessor<SharedCtx>, instance: Instance) -> GuestCallFuture<'a> {
        Box::pin(async move {
            match *self {
                Job::Typed { export, func, field, argument, reply } => {
                    let idx = export_func(accessor, instance, &export, func)?;
                    let f = accessor
                        .with(|mut a| instance.get_func(&mut a, idx))
                        .with_context(|| format!("{export}#{func} is not a function"))?;
                    let params = [Val::Record(vec![(field.to_string(), Val::String(argument))])];
                    let mut results = [Val::Bool(false)];
                    f.call_concurrent(accessor, &params, &mut results).await.map_err(wt)?;
                    let answer = typed_answer(&results[0]);
                    let refused = answer.is_err().then_some("tool");
                    let _ = reply.send(answer);
                    Ok(refused)
                }
                Job::List { reply } => {
                    let idx = export_func(accessor, instance, TOOLSET_EXPORT, "list")?;
                    let f = accessor
                        .with(|mut a| {
                            instance.get_typed_func::<(), (Result<Vec<ToolDef>, ListError>,)>(&mut a, &idx)
                        })
                        .map_err(wt)?;
                    let (r,) = f.call_concurrent(accessor, ()).await.map_err(wt)?;
                    let refused = r.is_err().then_some("toolset");
                    let _ = reply.send(r);
                    Ok(refused)
                }
                Job::Call { request, reply } => {
                    let idx = export_func(accessor, instance, TOOLSET_EXPORT, "call")?;
                    let f = accessor
                        .with(|mut a| {
                            instance.get_typed_func::<(ToolCall,), (Result<ToolResponse, CallError>,)>(&mut a, &idx)
                        })
                        .map_err(wt)?;
                    let (r,) = f.call_concurrent(accessor, (request,)).await.map_err(wt)?;
                    let refused = r.is_err().then_some("toolset");
                    let _ = reply.send(r);
                    Ok(refused)
                }
            }
        })
    }
}

/// A typed tool's `result<string, error>`: the text, or the error as
/// `{"case": payload}`.
fn typed_answer(v: &Val) -> Result<String, String> {
    let text = |v: &Val| match v {
        Val::String(s) => Some(s.clone()),
        Val::Option(Some(inner)) => match &**inner {
            Val::String(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    };
    match v {
        Val::Result(Ok(Some(ok))) => Ok(text(ok).unwrap_or_default()),
        Val::Result(Ok(None)) => Ok(String::new()),
        Val::Result(Err(Some(err))) => Err(match &**err {
            Val::Variant(case, payload) => variant(case, payload.as_deref().and_then(text)),
            other => variant("other", text(other)),
        }),
        _ => Err(variant("other", Some("the tool returned something other than a result".into()))),
    }
}

/// A WIT variant case as JSON: `{"case": payload}`.
fn variant(case: &str, payload: Option<String>) -> String {
    let mut map = serde_json::Map::new();
    map.insert(case.to_string(), payload.map_or(Value::Null, Value::String));
    Value::Object(map).to_string()
}

impl Toolbox {
    fn snapshot(&self, workload: &str) -> Option<(Vec<Arc<Typed>>, Vec<Arc<DispatchTarget>>)> {
        let offers = self.offers.lock().ok()?;
        let o = offers.get(workload)?;
        Some((o.typed.clone(), o.toolsets.clone()))
    }

    async fn list(&self, workload: &str) -> Result<Vec<ToolDef>, ListError> {
        let (typed, toolsets) = self
            .snapshot(workload)
            .ok_or_else(|| ListError::Unavailable("no tools are attached to this workload".into()))?;
        let mut defs: Vec<ToolDef> = typed.iter().map(|t| t.row.def()).collect();
        let mut routes = HashMap::new();
        for (i, target) in toolsets.iter().enumerate() {
            // A toolset that can't list its tools now is left out rather
            // than failing the others too.
            let (tx, rx) = oneshot::channel();
            if target.dispatch(Job::List { reply: tx }).await.is_err() {
                continue;
            }
            if let Ok(Ok(more)) = rx.await {
                for d in more {
                    if !defs.iter().any(|e| e.name == d.name) {
                        routes.insert(d.name.clone(), i);
                        defs.push(d);
                    }
                }
            }
        }
        if let Ok(mut offers) = self.offers.lock()
            && let Some(o) = offers.get_mut(workload)
        {
            o.routes = routes;
        }
        Ok(defs)
    }

    async fn call(&self, workload: &str, request: ToolCall) -> Result<ToolResponse, CallError> {
        let unknown = |name: &str| {
            CallError::NotDispatched(DispatchError::UnknownTool(format!("no tool named {name} in this workload")))
        };
        let (typed, toolsets) = self.snapshot(workload).ok_or_else(|| unknown(&request.name))?;
        if let Some(t) = typed.iter().find(|t| t.row.name == request.name) {
            return call_typed(t, &request).await;
        }
        let mut route = self.route(workload, &request.name);
        if route.is_none() {
            // Not seen at the last list: list again before refusing.
            let _ = self.list(workload).await;
            route = self.route(workload, &request.name);
        }
        let Some(target) = route.and_then(|i| toolsets.get(i)) else {
            return Err(unknown(&request.name));
        };
        let (tx, rx) = oneshot::channel();
        target
            .dispatch(Job::Call { request, reply: tx })
            .await
            .map_err(|e| CallError::OutcomeUnknown(OutcomeError::Upstream(format!("{e:#}"))))?;
        rx.await
            .map_err(|_| CallError::OutcomeUnknown(OutcomeError::Upstream("the toolset gave no answer".into())))?
    }

    fn route(&self, workload: &str, name: &str) -> Option<usize> {
        self.offers.lock().ok()?.get(workload)?.routes.get(name).copied()
    }
}

async fn call_typed(t: &Typed, request: &ToolCall) -> Result<ToolResponse, CallError> {
    let field = t.row.argument.0;
    let argument = match &request.arguments {
        ToolArguments::Json(j) => serde_json::from_str::<Value>(j)
            .ok()
            .and_then(|v| v.get(field).and_then(Value::as_str).map(String::from)),
        ToolArguments::Text(s) => Some(s.clone()),
    }
    .ok_or_else(|| CallError::NotDispatched(DispatchError::InvalidArguments(format!("missing argument {field:?}"))))?;
    let (tx, rx) = oneshot::channel();
    let job = Job::Typed { export: t.export.clone(), func: t.row.func, field, argument, reply: tx };
    t.target
        .dispatch(job)
        .await
        .map_err(|e| CallError::OutcomeUnknown(OutcomeError::Upstream(format!("{e:#}"))))?;
    let (text, is_error) = match rx.await {
        Ok(Ok(text)) => (text, false),
        Ok(Err(json)) => (json, true),
        Err(_) => return Err(CallError::OutcomeUnknown(OutcomeError::Upstream("the tool gave no answer".into()))),
    };
    Ok(ToolResponse { output: vec![ToolOutput::Text(text)], structured: None, is_error })
}

/// The calling workload and this plugin, from a host function's store.
fn caller<U>(accessor: &Accessor<U, SharedCtx>) -> (Arc<Toolbox>, String) {
    accessor.with(|mut access| {
        let ctx = access.get();
        (ctx.ctx.get_plugin::<Toolbox>(PLUGIN_ID), ctx.ctx.workload_id.to_string())
    })
}

impl tools::Host for ActiveCtx<'_> {}

impl<U> tools::HostWithStore<U> for SharedCtx {
    async fn list(accessor: &Accessor<U, Self>) -> wasmtime::Result<Result<Vec<ToolDef>, ListError>> {
        let (plugin, workload) = caller(accessor);
        Ok(plugin.list(&workload).await)
    }

    async fn call(
        accessor: &Accessor<U, Self>,
        request: ToolCall,
    ) -> wasmtime::Result<Result<ToolResponse, CallError>> {
        let (plugin, workload) = caller(accessor);
        Ok(plugin.call(&workload, request).await)
    }
}

/// The exact name of each `cosmonic:agent` interface the world imports, by
/// interface: `inference-types` to `cosmonic:agent/inference-types@0.4.1`.
fn agent_imports(world: &WitWorld) -> Vec<(String, String)> {
    world
        .imports
        .iter()
        .filter(|i| i.namespace == "cosmonic" && i.package == "agent")
        .flat_map(|i| {
            let v = i.version.as_ref().map(|v| format!("@{v}")).unwrap_or_default();
            i.interfaces.iter().map(move |n| (n.clone(), format!("cosmonic:agent/{n}{v}")))
        })
        .collect()
}

#[async_trait::async_trait]
impl HostPlugin for Toolbox {
    fn id(&self) -> &'static str {
        PLUGIN_ID
    }

    fn world(&self) -> WitWorld {
        WitWorld {
            imports: HashSet::from([
                WitInterface::from("cosmonic:agent/inference-types,tool-types,tools@0.4.1"),
                WitInterface::from("cosmonic:agent/inference-types@0.3.0"),
            ]),
            exports: HashSet::from([WitInterface::from("workshop:toolbox/curl,sqlite,gh@0.4.0")]),
        }
    }

    async fn on_workload_item_bind<'a>(
        &self,
        item: &mut WorkloadItem<'a>,
        _interfaces: WitInterfaces<'_>,
    ) -> anyhow::Result<()> {
        let imports = agent_imports(&item.world());
        for (iface, name) in &imports {
            match iface.as_str() {
                // Records and variants only: an empty instance links them.
                "inference-types" | "tool-types" => {
                    item.linker()?.instance(name)?;
                }
                "tools" => tools::add_to_linker::<_, SharedCtx>(item.linker()?, extract_active_ctx)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Find the workload's tools, once, before it starts.
    async fn on_workload_resolved(&self, workload: &ResolvedWorkload, _component_id: &str) -> anyhow::Result<()> {
        if self.offers.lock().map(|o| o.contains_key(workload.id())).unwrap_or(false) {
            return Ok(());
        }
        // The typed tools this workload's manifest offers.
        let declared: HashSet<String> = workload
            .host_interfaces()
            .iter()
            .filter(|i| i.namespace == "workshop" && i.package == "toolbox")
            .flat_map(|i| i.interfaces.iter().cloned())
            .collect();
        let mut found: Vec<(String, String)> = Vec::new();
        {
            let components = workload.components();
            let components = components.read().await;
            for (id, c) in components.iter() {
                for (export, _) in c.component_exports()? {
                    found.push((id.to_string(), export));
                }
            }
        }
        let mut offer = Offer::default();
        for (id, export) in found {
            if export == TOOLSET_EXPORT {
                let target = workload.dispatch_target(&id, PLUGIN_ID).await?;
                offer.toolsets.push(Arc::new(target));
                continue;
            }
            let Some(iface) = export.strip_prefix("workshop:toolbox/").map(|s| s.split('@').next().unwrap_or(s))
            else {
                continue;
            };
            if let Some(row) = TABLE.iter().find(|r| r.interface == iface && declared.contains(iface)) {
                let target = workload.dispatch_target(&id, PLUGIN_ID).await?;
                offer.typed.push(Arc::new(Typed { row, export, target }));
            }
        }
        tracing::info!(
            workload = workload.id(),
            typed = ?offer.typed.iter().map(|t| t.row.name).collect::<Vec<_>>(),
            toolsets = offer.toolsets.len(),
            "toolbox attached"
        );
        if let Ok(mut offers) = self.offers.lock() {
            offers.insert(workload.id().to_string(), offer);
        }
        Ok(())
    }

    async fn on_workload_unbind(&self, workload_id: &str, _interfaces: WitInterfaces<'_>) -> anyhow::Result<()> {
        if let Ok(mut offers) = self.offers.lock() {
            offers.remove(workload_id);
        }
        Ok(())
    }
}
