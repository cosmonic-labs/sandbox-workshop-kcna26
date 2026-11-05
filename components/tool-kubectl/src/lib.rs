//! kubectl: the real kube-rs client in a component.
//!
//! Every request goes through `workshop:toolbox/kube-api`, which the kube-api
//! host component plugin serves with its own service account token. This
//! component imports no outgoing HTTP, no sockets and no secrets, so it cannot
//! reach anything else or read the token. RBAC on that service account decides
//! what the requests may read.
//!
//! kube's client needs a tokio runtime for its request buffer, and tokio's
//! networking features only compile for wasm with `--cfg tokio_unstable`
//! (.cargo/config.toml here, so the flag reaches this crate alone). Nothing
//! calls that networking code, and the component imports no sockets.
//!
//! The command line follows Go kubectl: the same flags in all their forms, the
//! same table columns, `-o json|yaml|name|wide`, `describe` with kubectl's
//! describers, `get --raw`, `delete --raw`, `auth can-i` and the same error
//! lines. The plugin always asks the API server for JSON, so the server's own
//! table format isn't available: the tables and descriptions are printed
//! here, as kubectl prints them.

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/kubectl-tool", generate_all });
}

mod describe;
mod table;
mod yaml;

use std::convert::Infallible;

use bindings::exports::workshop::toolbox::kubectl::{Error, Guest, RunArgs};
use bindings::workshop::toolbox::kube_api as api;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, Secret, Service};
use kube::api::{Api, ApiResource, DynamicObject, GetParams, ListParams, LogParams, PostParams};
use kube::core::{Request, Resource};
use kube::Client;
use serde_json::{Value, json};

struct Component;

impl Guest for Component {
    async fn run(args: RunArgs) -> Result<String, Error> {
        run_blocking(&args.arguments)
    }
}

/// Run one kubectl command line to completion on a single-threaded tokio
/// runtime (kube's client needs one for its request buffer).
fn run_blocking(cmd: &str) -> Result<String, Error> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|e| Error::Other(Some(format!("tokio runtime: {e}"))))?;
    rt.block_on(run(cmd))
}

/// The transport kube's client sends through: the kube-api capability. The call
/// is made blocking (`wit_bindgen::block_on`), so the future tokio polls is
/// always ready and tokio never has to park.
async fn via_capability(
    req: http::Request<kube::client::Body>,
) -> Result<http::Response<Full<Bytes>>, Infallible> {
    let method = req.method().to_string();
    let pq = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
    let body = req.into_body().collect().await.map(|b| b.to_bytes().to_vec()).unwrap_or_default();
    let (status, body) = match wit_bindgen::block_on(api::send(method, pq, body)) {
        Ok(r) => (r.status, r.body),
        Err(e) => (502, format!(r#"{{"kind":"Status","apiVersion":"v1","status":"Failure","message":{},"reason":"BadGateway","code":502}}"#, serde_json::Value::from(e)).into_bytes()),
    };
    Ok(http::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap())
}

/// Commands that need streaming, a connection upgrade or a local credential,
/// none of which the capability has.
const UNSUPPORTED: &[&str] = &["exec", "port-forward", "attach", "cp", "proxy", "run", "debug", "config"];

/// Commands that change the cluster. This tool only reads, so it does not
/// implement them; `delete --raw` exists to show RBAC refusing a write.
const MUTATING: &[&str] = &[
    "apply", "create", "delete", "edit", "patch", "replace", "scale", "label", "annotate", "set",
    "rollout", "autoscale", "expose", "cordon", "uncordon", "drain", "taint",
];

// ---------------------------------------------------------------------------
// The command line.
// ---------------------------------------------------------------------------

/// The flags this kubectl knows: long name, shorthand, takes a value.
const FLAGS: &[(&str, Option<char>, bool)] = &[
    ("namespace", Some('n'), true),
    ("all-namespaces", Some('A'), false),
    ("output", Some('o'), true),
    ("selector", Some('l'), true),
    ("raw", None, true),
    ("watch", Some('w'), false),
    ("container", Some('c'), true),
    ("tail", None, true),
    ("since", None, true),
    ("previous", Some('p'), false),
    ("timestamps", None, false),
    ("follow", Some('f'), false),
    ("show-events", None, false),
];

/// Which flags each command accepts, besides `--namespace`, which all do. A
/// flag outside its command's list is an unknown flag, as in kubectl.
fn accepts(verb: &str, flag: &str) -> bool {
    let list: &[&str] = match verb {
        "get" => &["all-namespaces", "output", "selector", "raw", "watch"],
        "logs" => &["container", "tail", "since", "previous", "timestamps", "follow"],
        "describe" => &["all-namespaces", "selector", "show-events"],
        "auth" => &["all-namespaces"],
        "delete" => &["raw"],
        _ => &[],
    };
    flag == "namespace" || list.contains(&flag)
}

/// One parsed command line: the words, and each flag given with its value
/// (`"true"` for a flag without one).
struct Cmd {
    words: Vec<String>,
    flags: Vec<(&'static str, String)>,
}

impl Cmd {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }
    fn on(&self, name: &str) -> bool {
        self.flag(name).is_some_and(|v| v != "false")
    }
}

/// Split a command line the way a shell would hand it to kubectl: on
/// whitespace, with single or double quotes holding a word together.
fn words(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quote, mut any) = (None, false);
    for c in cmd.chars() {
        match (quote, c) {
            (None, '\'' | '"') => (quote, any) = (Some(c), true),
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if any {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (_, c) => (cur.push(c), any = true).1,
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// Parse flags anywhere on the line, in every form kubectl takes: `-n x`,
/// `-nx`, `-n=x`, `--namespace x`, `--namespace=x`, and bundled booleans
/// such as `-pA`. The error is kubectl's own wording.
fn parse(cmd: &str) -> Result<Cmd, String> {
    let mut it = words(cmd).into_iter();
    let (mut words, mut flags) = (Vec::new(), Vec::new());
    while let Some(w) = it.next() {
        if w == "--" {
            words.extend(it.by_ref());
        } else if let Some(long) = w.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let Some(&(flag, _, takes)) = FLAGS.iter().find(|f| f.0 == name) else {
                return Err(format!("unknown flag: --{name}"));
            };
            let value = match (takes, inline) {
                (_, Some(v)) => v,
                (true, None) => it.next().ok_or(format!("flag needs an argument: --{name}"))?,
                (false, None) => "true".into(),
            };
            flags.push((flag, value));
        } else if w.len() > 1 && w.starts_with('-') {
            for (i, c) in w[1..].char_indices() {
                let Some(&(flag, _, takes)) = FLAGS.iter().find(|f| f.1 == Some(c)) else {
                    return Err(format!("unknown shorthand flag: '{c}' in {w}"));
                };
                if !takes {
                    flags.push((flag, "true".into()));
                    continue;
                }
                let rest = &w[1 + i + c.len_utf8()..];
                let value = match rest.strip_prefix('=').unwrap_or(rest) {
                    "" => it.next().ok_or(format!("flag needs an argument: '{c}' in -{c}"))?,
                    v => v.to_string(),
                };
                flags.push((flag, value));
                break;
            }
        } else {
            words.push(w);
        }
    }
    Ok(Cmd { words, flags })
}

/// What `run` returns when the line itself is wrong, worded as kubectl.
fn usage(verb: &str, msg: impl std::fmt::Display) -> Error {
    Error::InvalidArgument(format!("error: {msg}\nSee 'kubectl {verb} --help' for usage."))
}

async fn run(line: &str) -> Result<String, Error> {
    let cmd = parse(line);
    // The verb is the first word; with a bad flag, kubectl still names it.
    let verb = match &cmd {
        Ok(c) => c.words.first().cloned().unwrap_or_default(),
        Err(_) => words(line).into_iter().find(|w| !w.starts_with('-')).unwrap_or_default(),
    };
    let cmd = cmd.map_err(|e| usage(&verb, e))?;
    if verb.is_empty() {
        return Err(Error::InvalidArgument(
            "usage: kubectl get|describe|logs|auth can-i|version|api-versions [args] [-n ns | -A]".into(),
        ));
    }
    if UNSUPPORTED.contains(&verb.as_str()) {
        return Err(Error::AccessDenied(format!(
            "\"{verb}\" is not available: this kubectl sends plain API requests through the kube-api capability, which has no streaming, upgrade or local credential"
        )));
    }
    if MUTATING.contains(&verb.as_str()) && !(verb == "delete" && cmd.flag("raw").is_some()) {
        return Err(Error::AccessDenied(format!(
            "\"{verb}\" is not implemented in this kubectl tool, which only reads. To see RBAC refuse a write, send the request yourself with delete --raw <path>"
        )));
    }
    if let Some((name, _)) = cmd.flags.iter().find(|(n, _)| !accepts(&verb, n)) {
        return Err(usage(&verb, format!("unknown flag: --{name}")));
    }
    // Like a kubeconfig context's namespace: the one this workload's
    // service account may read (k8s/kube.yaml).
    let ns = cmd.flag("namespace").unwrap_or("demo").to_string();
    let args = &cmd.words[1..];

    // `--raw PATH`: send exactly this request through the capability, to show
    // what the capability itself allows, past this tool's own checks. The
    // plugin forwards any method; RBAC is what answers.
    if let Some(path) = cmd.flag("raw") {
        let method = if verb == "delete" { "DELETE" } else { "GET" };
        if !path.starts_with('/') {
            return Err(Error::InvalidArgument(format!(
                "error: --raw must be a valid URL path: parse \"{path}\": invalid URI for request"
            )));
        }
        return match wit_bindgen::block_on(api::send(method.into(), path.into(), Vec::new())) {
            Ok(r) if r.status < 400 => Ok(String::from_utf8_lossy(&r.body).into_owned()),
            Ok(r) => Err(status_error(r.status, &r.body)),
            Err(e) => Err(Error::Network(e)),
        };
    }

    let client = Client::new(tower::service_fn(via_capability), ns.clone());
    let result = match verb.as_str() {
        "get" => get(&client, &cmd, args, &ns).await,
        "describe" => describe_cmd(&client, &cmd, args, &ns).await,
        "logs" => logs(&client, &cmd, args, &ns).await,
        "auth" => auth(&client, &cmd, args, &ns).await,
        "version" => version(&client).await,
        "api-versions" => api_versions(&client).await,
        other => return Err(Error::InvalidArgument(format!("error: unknown command \"{other}\" for \"kubectl\""))),
    };
    result.map_err(|e| match e {
        Fail::Kube(e) => kube_error(e),
        Fail::Tool(e) => e,
    })
}

/// A command fails either with an answer from the API server, or with an
/// error of this tool's own, already worded.
enum Fail {
    Kube(kube::Error),
    Tool(Error),
}

impl From<kube::Error> for Fail {
    fn from(e: kube::Error) -> Self {
        Fail::Kube(e)
    }
}

type Out = Result<String, Fail>;

fn invalid(msg: impl Into<String>) -> Fail {
    Fail::Tool(Error::InvalidArgument(msg.into()))
}

/// kubectl's line for an error from the API server. A refusal (401 or 403) is
/// `access-denied`; the rest are `other`.
fn server_error(code: u16, reason: &str, message: &str) -> Error {
    let line = format!("Error from server ({reason}): {message}");
    match code {
        // 502 is this tool's own marker for "the capability could not send it".
        502 if reason == "BadGateway" => Error::Network(message.into()),
        401 | 403 => Error::AccessDenied(line),
        _ => Error::Other(Some(line)),
    }
}

fn kube_error(e: kube::Error) -> Error {
    match e {
        kube::Error::Api(s) => server_error(s.code, &s.reason, &s.message),
        e => Error::Other(Some(format!("error: {e}"))),
    }
}

/// The error for a raw response body, which is usually a Status object.
fn status_error(code: u16, body: &[u8]) -> Error {
    let s: Value = serde_json::from_slice(body).unwrap_or_default();
    match s.get("message").and_then(Value::as_str) {
        Some(msg) => server_error(code, s["reason"].as_str().unwrap_or("Unknown"), msg),
        None => server_error(code, "Unknown", String::from_utf8_lossy(body).trim()),
    }
}

/// Send one request through kube's client and parse the JSON answer.
async fn fetch(client: &Client, req: Result<http::Request<Vec<u8>>, kube::core::request::Error>) -> Result<Value, Fail> {
    let req = req.map_err(|e| Fail::Kube(kube::Error::BuildRequest(e)))?;
    Ok(client.request::<Value>(req).await?)
}

// ---------------------------------------------------------------------------
// Resource types.
// ---------------------------------------------------------------------------

/// A resource type this kubectl knows, with what it needs to print it.
pub struct Kind {
    pub ar: ApiResource,
    pub namespaced: bool,
}

impl Kind {
    /// `pod`, or `deployment.apps`: the prefix of `-o name`.
    pub fn short(&self) -> String {
        let kind = self.ar.kind.to_lowercase();
        if self.ar.group.is_empty() { kind } else { format!("{kind}.{}", self.ar.group) }
    }
}

/// The types this workshop's RBAC lets agent-reader read, and the ones it
/// refuses, so a refusal comes from the API server and not from this tool.
fn kind(name: &str) -> Option<Kind> {
    let name = name.to_lowercase();
    // `deployments.apps` names the group too.
    let base = name.split('.').next().unwrap_or_default();
    let (ar, namespaced) = match base {
        "pods" | "pod" | "po" => (ApiResource::erase::<Pod>(&()), true),
        "deployments" | "deployment" | "deploy" => (ApiResource::erase::<Deployment>(&()), true),
        "services" | "service" | "svc" => (ApiResource::erase::<Service>(&()), true),
        "secrets" | "secret" => (ApiResource::erase::<Secret>(&()), true),
        "configmaps" | "configmap" | "cm" => (ApiResource::erase::<ConfigMap>(&()), true),
        "namespaces" | "namespace" | "ns" => (ApiResource::erase::<Namespace>(&()), false),
        "workloaddeployments" | "workloaddeployment" => (
            ApiResource {
                group: "runtime.wasmcloud.dev".into(),
                version: "v1alpha1".into(),
                api_version: "runtime.wasmcloud.dev/v1alpha1".into(),
                kind: "WorkloadDeployment".into(),
                plural: "workloaddeployments".into(),
            },
            true,
        ),
        _ => return None,
    };
    match name.split_once('.') {
        Some((_, group)) if group != ar.group => None,
        _ => Some(Kind { ar, namespaced }),
    }
}

fn unknown_type(name: &str) -> Fail {
    invalid(format!("error: the server doesn't have a resource type \"{name}\""))
}

// ---------------------------------------------------------------------------
// get
// ---------------------------------------------------------------------------

/// `get TYPE[,TYPE...] [NAME...]` or `get TYPE/NAME...`, printed as a table,
/// or with `-o json|yaml|name|wide`.
async fn get(client: &Client, cmd: &Cmd, args: &[String], ns: &str) -> Out {
    if cmd.on("watch") {
        return Err(Fail::Tool(Error::AccessDenied(
            "--watch is not available: the kube-api capability returns one answer per request, with no streaming".into(),
        )));
    }
    let output = cmd.flag("output").unwrap_or("");
    if !["", "json", "yaml", "name", "wide"].contains(&output) {
        return Err(invalid(format!(
            "error: unable to match a printer suitable for the output format \"{output}\", allowed formats are: json,name,wide,yaml"
        )));
    }
    let Some(first) = args.first() else {
        return Err(invalid(
            "You must specify the type of resource to get. Use \"kubectl api-resources\" for a complete list of supported resources.\n\nerror: Required resource not specified.",
        ));
    };
    // What to fetch: each type, with the names asked for (none: list them).
    let mut wanted: Vec<(Kind, Vec<String>)> = Vec::new();
    if first.contains('/') {
        for a in args {
            let Some((t, name)) = a.split_once('/') else {
                return Err(invalid("error: there is no need to specify a resource type as a separate argument when passing arguments in resource/name form"));
            };
            let k = kind(t).ok_or_else(|| unknown_type(t))?;
            match wanted.iter_mut().find(|(w, _)| w.ar.plural == k.ar.plural) {
                Some((_, names)) => names.push(name.into()),
                None => wanted.push((k, vec![name.into()])),
            }
        }
    } else {
        for t in first.split(',') {
            wanted.push((kind(t).ok_or_else(|| unknown_type(t))?, args[1..].to_vec()));
        }
    }
    let all = cmd.on("all-namespaces");
    let mut lp = ListParams::default();
    if let Some(sel) = cmd.flag("selector") {
        lp = lp.labels(sel);
    }

    // Fetch everything first, as kubectl does, then print.
    let mut found: Vec<(Kind, Vec<Value>)> = Vec::new();
    let single = wanted.len() == 1 && wanted[0].1.len() == 1;
    for (k, names) in wanted {
        let mut items = Vec::new();
        if names.is_empty() {
            let scope = (k.namespaced && !all).then_some(ns);
            let req = Request::new(DynamicObject::url_path(&k.ar, scope));
            let list = fetch(client, req.list(&lp)).await?;
            items = list["items"].as_array().cloned().unwrap_or_default();
            // A list's items carry no apiVersion or kind; kubectl adds them.
            for item in &mut items {
                item["apiVersion"] = json!(k.ar.api_version);
                item["kind"] = json!(k.ar.kind);
            }
        } else {
            for name in &names {
                let req = Request::new(DynamicObject::url_path(&k.ar, k.namespaced.then_some(ns)));
                items.push(fetch(client, req.get(name, &GetParams::default())).await?);
            }
        }
        found.push((k, items));
    }

    // kubectl hides managedFields unless asked (--show-managed-fields).
    for (_, items) in &mut found {
        for item in items {
            if let Some(meta) = item.get_mut("metadata").and_then(Value::as_object_mut) {
                meta.remove("managedFields");
            }
        }
    }
    let count: usize = found.iter().map(|(_, items)| items.len()).sum();
    match output {
        "json" | "yaml" => {
            let v = if single {
                found[0].1[0].clone()
            } else {
                let items: Vec<Value> = found.into_iter().flat_map(|(_, items)| items).collect();
                json!({ "apiVersion": "v1", "items": items, "kind": "List", "metadata": { "resourceVersion": "" } })
            };
            Ok(if output == "json" { to_json(&v) } else { yaml::to_string(&v) })
        }
        _ if count == 0 => Ok(if all || !found.iter().any(|(k, _)| k.namespaced) {
            "No resources found\n".into()
        } else {
            format!("No resources found in {ns} namespace.\n")
        }),
        "name" => Ok(found
            .iter()
            .flat_map(|(k, items)| items.iter().map(move |o| format!("{}/{}\n", k.short(), str_at(o, "/metadata/name"))))
            .collect()),
        _ => {
            // Several types: one table each, names written as type/name.
            let prefix = found.len() > 1;
            let now = table::now();
            let tables: Vec<String> = found
                .iter()
                .filter(|(_, items)| !items.is_empty())
                .map(|(k, items)| table::print(k, items, all, output == "wide", prefix, now))
                .collect();
            Ok(tables.join("\n"))
        }
    }
}

/// JSON as kubectl prints it: four-space indents, keys in order, and `<`, `>`
/// and `&` escaped, as Go's encoder does.
fn to_json(v: &Value) -> String {
    use serde::Serialize;
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, serde_json::ser::PrettyFormatter::with_indent(b"    "));
    let _ = v.serialize(&mut ser);
    let s = String::from_utf8(buf).unwrap_or_default();
    s.replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026") + "\n"
}

/// The string at a JSON pointer, or "".
pub fn str_at<'a>(v: &'a Value, ptr: &str) -> &'a str {
    v.pointer(ptr).and_then(Value::as_str).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------------

/// `describe TYPE [NAME_PREFIX]`, `describe TYPE NAME...` or `describe
/// TYPE/NAME...`, in kubectl's order: fetch the objects (a name that isn't
/// found, given as `TYPE NAME`, is tried as a prefix of every name), then
/// describe each, with the requests its describer makes. Events are shown
/// for a single object and left out for several, unless --show-events says
/// otherwise; a refused events list prints as `Events: <none>`, and a
/// refused replica set or endpoint slice list leaves that line out, as
/// kubectl does.
async fn describe_cmd(client: &Client, cmd: &Cmd, args: &[String], ns: &str) -> Out {
    let Some(first) = args.first() else {
        return Err(invalid(
            "error: You must specify the type of resource to describe. Use \"kubectl api-resources\" for a complete list of supported resources.",
        ));
    };
    let all = cmd.on("all-namespaces");
    let mut wanted: Vec<(Kind, Vec<String>)> = Vec::new();
    if first.contains('/') {
        for a in args {
            let Some((t, name)) = a.split_once('/') else {
                return Err(invalid("error: there is no need to specify a resource type as a separate argument when passing arguments in resource/name form"));
            };
            let k = kind(t).ok_or_else(|| unknown_type(t))?;
            match wanted.iter_mut().find(|(w, _)| w.ar.plural == k.ar.plural) {
                Some((_, names)) => names.push(name.into()),
                None => wanted.push((k, vec![name.into()])),
            }
        }
    } else {
        for t in first.split(',') {
            wanted.push((kind(t).ok_or_else(|| unknown_type(t))?, args[1..].to_vec()));
        }
    }
    let named = wanted.iter().any(|(_, names)| !names.is_empty());
    if named && all {
        return Err(invalid("error: a resource cannot be retrieved by name across all namespaces"));
    }
    if named && cmd.flag("selector").is_some() {
        return Err(invalid("error: name cannot be provided when a selector is specified"));
    }
    let mut lp = ListParams::default();
    if let Some(sel) = cmd.flag("selector") {
        lp = lp.labels(sel);
    }

    // Fetch every object, carrying on past errors as kubectl does.
    let mut infos: Vec<(&Kind, Value)> = Vec::new();
    let mut errors: Vec<Fail> = Vec::new();
    for (k, names) in &wanted {
        if names.is_empty() {
            let scope = (k.namespaced && !all).then_some(ns);
            let req = Request::new(DynamicObject::url_path(&k.ar, scope));
            match fetch(client, req.list(&lp)).await {
                Ok(list) => infos.extend(list["items"].as_array().into_iter().flatten().map(|o| (k, o.clone()))),
                Err(e) => errors.push(e),
            }
        } else {
            for name in names {
                let req = Request::new(DynamicObject::url_path(&k.ar, k.namespaced.then_some(ns)));
                match fetch(client, req.get(name, &GetParams::default())).await {
                    Ok(o) => infos.push((k, o)),
                    Err(e) => errors.push(e),
                }
            }
        }
    }
    let explicit = cmd.flag("show-events").map(|v| v != "false");
    let show = explicit.unwrap_or(errors.is_empty() && infos.len() <= 1);

    // `describe TYPE NAME` with no such NAME: describe every object of TYPE
    // whose name starts with it, or fail with the original error.
    let not_found = |e: &Fail| matches!(e, Fail::Kube(kube::Error::Api(s)) if s.code == 404);
    if args.len() == 2 && !first.contains('/') && wanted.len() == 1 && errors.first().is_some_and(not_found) {
        let k = &wanted[0].0;
        let req = Request::new(DynamicObject::url_path(&k.ar, k.namespaced.then_some(ns)));
        let list = fetch(client, req.list(&ListParams::default())).await?;
        let mut out = String::new();
        for o in list["items"].as_array().into_iter().flatten() {
            if str_at(o, "/metadata/name").starts_with(args[1].as_str()) {
                out.push_str(&describe_one(client, k, o, show).await?);
                out.push('\n');
            }
        }
        return if out.is_empty() { Err(errors.remove(0)) } else { Ok(out) };
    }

    let mut out = String::new();
    for (k, o) in &infos {
        match describe_one(client, k, o, show).await {
            Ok(s) if out.is_empty() => out.push_str(&s),
            Ok(s) => out.push_str(&format!("\n\n{s}")),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        if infos.is_empty() {
            return Ok(if all { "No resources found\n".into() } else { format!("No resources found in {ns} namespace.\n") });
        }
        return Ok(out);
    }
    // kubectl prints what it could describe, then each distinct error.
    let mut errors: Vec<Error> = errors
        .into_iter()
        .map(|e| match e {
            Fail::Kube(e) => kube_error(e),
            Fail::Tool(e) => e,
        })
        .collect();
    let mut lines: Vec<String> = Vec::new();
    for e in &errors {
        let line = error_text(e);
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    let text = format!("{out}{}", lines.join("\n"));
    Err(Fail::Tool(match errors.remove(0) {
        Error::AccessDenied(_) => Error::AccessDenied(text),
        Error::InvalidArgument(_) => Error::InvalidArgument(text),
        Error::Network(_) => Error::Network(text),
        _ => Error::Other(Some(text)),
    }))
}

fn error_text(e: &Error) -> String {
    match e {
        Error::AccessDenied(s) | Error::InvalidArgument(s) | Error::Network(s) | Error::Other(Some(s)) => s.clone(),
        _ => String::new(),
    }
}

/// One object through its describer, with the extra requests that describer
/// makes. Like kubectl's, a failed extra request mostly leaves its section
/// out; a ConfigMap's refused events, and a Namespace's refused quotas or
/// limit ranges, fail the describe.
async fn describe_one(client: &Client, k: &Kind, o: &Value, show_events: bool) -> Out {
    let ns = str_at(o, "/metadata/namespace");
    let name = str_at(o, "/metadata/name");
    let list = |path: String, lp: ListParams| async move {
        let items = fetch(client, Request::new(path).list(&lp)).await?;
        Ok::<Vec<Value>, Fail>(items["items"].as_array().cloned().unwrap_or_default())
    };
    let mut x = describe::Extra::default();
    if show_events && k.ar.kind != "Namespace" && k.ar.kind != "Secret" {
        // The pod describer leaves the kind out of its selector.
        let mut fields = format!("involvedObject.name={name},involvedObject.namespace={ns},involvedObject.uid={}", str_at(o, "/metadata/uid"));
        if k.ar.kind != "Pod" {
            fields.push_str(&format!(",involvedObject.kind={}", k.ar.kind));
        }
        let path = format!("/api/v1/namespaces/{ns}/events");
        x.events = Some(match list(path, ListParams::default().fields(&fields).limit(500)).await {
            Ok(items) => items,
            Err(e) if k.ar.kind == "ConfigMap" => return Err(e),
            Err(_) => Vec::new(),
        });
    }
    match k.ar.kind.as_str() {
        "Deployment" => {
            let selector = describe::selector(o.pointer("/spec/selector").unwrap_or(&Value::Null));
            let path = format!("/apis/apps/v1/namespaces/{ns}/replicasets");
            if let Ok(items) = list(path, ListParams::default().labels(&selector).limit(500)).await {
                let uid = str_at(o, "/metadata/uid");
                let rev = str_at(o, "/metadata/annotations/deployment.kubernetes.io~1revision");
                let (new, old): (Vec<Value>, Vec<Value>) = items
                    .into_iter()
                    .filter(|rs| {
                        rs.pointer("/metadata/ownerReferences")
                            .and_then(Value::as_array)
                            .is_some_and(|refs| refs.iter().any(|r| r["controller"] == json!(true) && str_at(r, "/uid") == uid))
                    })
                    .partition(|rs| str_at(rs, "/metadata/annotations/deployment.kubernetes.io~1revision") == rev);
                x.replica_sets = Some((old, new));
            }
        }
        "Service" => {
            let path = format!("/apis/discovery.k8s.io/v1/namespaces/{ns}/endpointslices");
            let lp = ListParams::default().labels(&format!("kubernetes.io/service-name={name}"));
            x.endpoint_slices = list(path, lp).await.unwrap_or_default();
        }
        "Namespace" => {
            let quiet_404 = |r: Result<Vec<Value>, Fail>| match r {
                Ok(items) => Ok(Some(items)),
                Err(Fail::Kube(kube::Error::Api(s))) if s.code == 404 => Ok(None),
                Err(e) => Err(e),
            };
            let lp = || ListParams::default().limit(500);
            x.quotas = quiet_404(list(format!("/api/v1/namespaces/{name}/resourcequotas"), lp()).await)?;
            x.limit_ranges = quiet_404(list(format!("/api/v1/namespaces/{name}/limitranges"), lp()).await)?;
        }
        _ => {}
    }
    Ok(describe::describe(&k.ar.kind, o, &x, table::now()))
}

// ---------------------------------------------------------------------------
// logs
// ---------------------------------------------------------------------------

/// `logs POD|pod/POD|deployment/NAME [-c container] [--tail N] [--since 5m]
/// [-p] [--timestamps]`. A deployment's logs come from the first of its pods
/// by name, as kubectl picks one pod for you.
async fn logs(client: &Client, cmd: &Cmd, args: &[String], ns: &str) -> Out {
    if cmd.on("follow") {
        return Err(Fail::Tool(Error::AccessDenied(
            "-f/--follow is not available: the kube-api capability returns one answer per request, with no streaming. Run logs again for newer lines".into(),
        )));
    }
    let Some(target) = args.first() else {
        return Err(invalid(
            "error: expected 'logs [-f] [-p] (POD | TYPE/NAME) [-c CONTAINER]'.\nPOD or TYPE/NAME is a required argument for the logs command",
        ));
    };
    // kubectl's own wording when the pod or deployment named isn't there.
    let not_found = |e: Fail| match e {
        Fail::Kube(kube::Error::Api(s)) if s.code == 404 => Fail::Tool(Error::Other(Some(format!(
            "error: error from server (NotFound): {} in namespace \"{ns}\"",
            s.message
        )))),
        e => e,
    };
    let pod_url = DynamicObject::url_path(&ApiResource::erase::<Pod>(&()), Some(ns));
    let pod_name = match target.split_once('/') {
        None => target.clone(),
        Some(("pod" | "pods" | "po", name)) => name.into(),
        Some(("deployment" | "deployments" | "deploy" | "deployment.apps" | "deployments.apps", name)) => {
            let url = DynamicObject::url_path(&ApiResource::erase::<Deployment>(&()), Some(ns));
            let deploy = fetch(client, Request::new(url).get(name, &GetParams::default())).await.map_err(not_found)?;
            let selector = deploy
                .pointer("/spec/selector/matchLabels")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default())).collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            let pods = fetch(client, Request::new(pod_url.clone()).list(&ListParams::default().labels(&selector))).await?;
            let mut names: Vec<&str> = pods["items"]
                .as_array()
                .map(|a| a.iter().map(|p| str_at(p, "/metadata/name")).collect())
                .unwrap_or_default();
            names.sort();
            match names.first() {
                Some(pod) => pod.to_string(),
                None => return Err(Fail::Tool(Error::Other(Some(format!("error: deployment/{name} has no pods"))))),
            }
        }
        Some((t, _)) => return Err(invalid(format!("error: cannot get logs from {t}: this kubectl reads logs from pods and deployments"))),
    };
    // Read the pod to pick its container, as kubectl does: the one asked for,
    // else the default-container annotation, else the first.
    let pod = fetch(client, Request::new(pod_url).get(&pod_name, &GetParams::default())).await.map_err(not_found)?;
    let containers: Vec<&str> = ["/spec/initContainers", "/spec/containers"]
        .iter()
        .filter_map(|p| pod.pointer(p).and_then(Value::as_array))
        .flatten()
        .map(|c| str_at(c, "/name"))
        .collect();
    let container = match cmd.flag("container").or(args.get(1).map(String::as_str)) {
        Some(c) if !containers.contains(&c) => {
            return Err(invalid(format!(
                "error: container {c} is not valid for pod {pod_name} out of: {}",
                containers.join(", ")
            )));
        }
        Some(c) => c.to_string(),
        None => match str_at(&pod, "/metadata/annotations/kubectl.kubernetes.io~1default-container") {
            "" => str_at(&pod, "/spec/containers/0/name").to_string(),
            c => c.to_string(),
        },
    };
    let mut lp = LogParams {
        container: Some(container),
        previous: cmd.on("previous"),
        timestamps: cmd.on("timestamps"),
        ..LogParams::default()
    };
    if let Some(tail) = cmd.flag("tail") {
        let n: i64 = tail
            .parse()
            .map_err(|_| invalid(format!("error: invalid argument \"{tail}\" for \"--tail\" flag: strconv.ParseInt: parsing \"{tail}\": invalid syntax")))?;
        // -1, kubectl's default, means every line.
        lp.tail_lines = (n >= 0).then_some(n);
    }
    if let Some(since) = cmd.flag("since") {
        let secs = duration_secs(since)
            .ok_or_else(|| invalid(format!("error: invalid argument \"{since}\" for \"--since\" flag: time: invalid duration \"{since}\"")))?;
        lp.since_seconds = Some(secs.max(1));
    }
    Ok(Api::<Pod>::namespaced(client.clone(), ns).logs(&pod_name, &lp).await?)
}

/// A Go duration such as `30s`, `5m` or `1h30m`, in whole seconds.
fn duration_secs(s: &str) -> Option<i64> {
    let (mut total, mut num, mut any) = (0f64, String::new(), false);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let unit = match (c, chars.peek()) {
            ('m', Some('s')) => (chars.next(), 0.001).1,
            ('h', _) => 3600.0,
            ('m', _) => 60.0,
            ('s', _) => 1.0,
            _ => return None,
        };
        total += num.parse::<f64>().ok()? * unit;
        (num, any) = (String::new(), true);
    }
    (any && num.is_empty()).then_some(total as i64)
}

// ---------------------------------------------------------------------------
// auth, version, api-versions
// ---------------------------------------------------------------------------

/// `auth can-i VERB TYPE[/NAME]` or `auth can-i VERB /path`: ask the API
/// server with a SelfSubjectAccessReview, which any identity may create, and
/// print kubectl's `yes` or `no`. It shows what RBAC allows without trying.
async fn auth(client: &Client, cmd: &Cmd, args: &[String], ns: &str) -> Out {
    if args.first().map(String::as_str) != Some("can-i") {
        return Err(invalid("error: this kubectl offers \"auth can-i\" only"));
    }
    let (Some(verb), Some(what), None) = (args.get(1), args.get(2), args.get(3)) else {
        return Err(invalid(
            "error: you must specify two arguments: verb resource or verb resource/resourceName.\nSee 'kubectl auth can-i -h' for help and examples.",
        ));
    };
    let attrs = if what.starts_with('/') {
        json!({ "nonResourceAttributes": { "path": what, "verb": verb } })
    } else {
        let (res, name) = what.split_once('/').unwrap_or((what, ""));
        // A known type gets its plural and group; any other is asked as named.
        let (resource, group) = match kind(res) {
            Some(k) => (k.ar.plural, k.ar.group),
            None => match res.split_once('.') {
                Some((r, g)) => (r.to_string(), g.to_string()),
                None => (res.to_string(), String::new()),
            },
        };
        let namespace = if cmd.on("all-namespaces") { "" } else { ns };
        json!({ "resourceAttributes": {
            "namespace": namespace, "verb": verb, "group": group, "resource": resource, "name": name,
        }})
    };
    let review = json!({
        "apiVersion": "authorization.k8s.io/v1",
        "kind": "SelfSubjectAccessReview",
        "spec": attrs,
    });
    let req = Request::new("/apis/authorization.k8s.io/v1/selfsubjectaccessreviews")
        .create(&PostParams::default(), review.to_string().into_bytes());
    let answer = fetch(client, req).await?;
    let status = &answer["status"];
    if status["allowed"].as_bool() == Some(true) {
        return Ok("yes\n".into());
    }
    // kubectl prints "no" on stdout, with the server's reason when it gives one.
    let mut out = "no".to_string();
    for key in ["reason", "evaluationError"] {
        if let Some(s) = status[key].as_str().filter(|s| !s.is_empty()) {
            out.push_str(&format!(" - {s}"));
        }
    }
    Ok(out + "\n")
}

/// `version`: this tool's version, and the API server's (`GET /version`).
/// (The scripted model reads tool output for commands, so the client line
/// avoids the word kubectl followed by a space.)
async fn version(client: &Client) -> Out {
    let server = fetch(client, Ok(http::Request::get("/version").body(Vec::new()).unwrap())).await?;
    Ok(format!(
        "Client Version: v{} (workshop tool, kube-rs 5.0.0, k8s-openapi 0.29.0)\nServer Version: {}\n",
        env!("CARGO_PKG_VERSION"),
        str_at(&server, "/gitVersion"),
    ))
}

/// `api-versions`: every group/version the API server serves, one per line.
async fn api_versions(client: &Client) -> Out {
    let core = fetch(client, Ok(http::Request::get("/api").body(Vec::new()).unwrap())).await?;
    let groups = fetch(client, Ok(http::Request::get("/apis").body(Vec::new()).unwrap())).await?;
    let mut all: Vec<&str> = core["versions"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    for g in groups["groups"].as_array().into_iter().flatten() {
        all.extend(g["versions"].as_array().into_iter().flatten().map(|v| str_at(v, "/groupVersion")));
    }
    all.sort();
    all.dedup();
    Ok(all.iter().map(|v| format!("{v}\n")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_in_every_form() {
        for line in ["get pods -n x", "get pods -nx", "get pods -n=x", "get pods --namespace x", "get pods --namespace=x", "-n x get pods"] {
            let c = parse(line).unwrap();
            assert_eq!(c.words, ["get", "pods"], "{line}");
            assert_eq!(c.flag("namespace"), Some("x"), "{line}");
        }
        let c = parse("get pods -ojson -A").unwrap();
        assert_eq!((c.flag("output"), c.on("all-namespaces")), (Some("json"), true));
        assert_eq!(parse("get pods --foo").err().unwrap(), "unknown flag: --foo");
        assert_eq!(parse("get pods -x").err().unwrap(), "unknown shorthand flag: 'x' in -x");
        assert_eq!(parse("get pods -l 'app=a b'").unwrap().flag("selector"), Some("app=a b"));
    }

    #[test]
    fn go_durations() {
        assert_eq!(duration_secs("5m"), Some(300));
        assert_eq!(duration_secs("1h30m"), Some(5400));
        assert_eq!(duration_secs("5"), None);
        assert_eq!(duration_secs("x"), None);
    }
}

/// describe's request flow against canned API answers: the objects from a
/// kind cluster, with events and replica sets refused as RBAC refuses them
/// for agent-reader, against `kubectl --as=...:agent-reader describe`
/// (v1.37.1, TZ=UTC).
#[cfg(test)]
mod describe_flow {
    use super::*;

    fn pods() -> Vec<Value> {
        serde_json::from_str(include_str!("testdata/pods.json")).unwrap()
    }

    fn status(code: u16, reason: &str, message: String) -> (u16, String) {
        let s = json!({ "kind": "Status", "apiVersion": "v1", "metadata": {}, "status": "Failure",
            "message": message, "reason": reason, "code": code });
        (code, s.to_string())
    }

    async fn api(req: http::Request<kube::client::Body>) -> Result<http::Response<Full<Bytes>>, Infallible> {
        let path = req.uri().path().to_string();
        let forbidden = |what: &str, group: &str| {
            let m = format!(
                "{what} is forbidden: User \"system:serviceaccount:wasmcloud:agent-reader\" cannot list resource \"{what}\" in API group \"{group}\" in the namespace \"demo\""
            );
            status(403, "Forbidden", m)
        };
        let (code, body) = match path.as_str() {
            "/api/v1/namespaces/demo/pods" => (200, json!({ "kind": "PodList", "apiVersion": "v1", "metadata": {}, "items": pods() }).to_string()),
            "/apis/apps/v1/namespaces/demo/deployments/hello" => (200, include_str!("testdata/deployment.json").to_string()),
            "/api/v1/namespaces/demo/events" => forbidden("events", ""),
            "/apis/apps/v1/namespaces/demo/replicasets" => forbidden("replicasets", "apps"),
            p => match p.strip_prefix("/api/v1/namespaces/demo/pods/") {
                Some(name) => match pods().into_iter().find(|o| str_at(o, "/metadata/name") == name) {
                    Some(o) => (200, o.to_string()),
                    None => status(404, "NotFound", format!("pods \"{name}\" not found")),
                },
                None => status(404, "NotFound", "the server could not find the requested resource".into()),
            },
        };
        Ok(http::Response::builder().status(code).body(Full::new(Bytes::from(body))).unwrap())
    }

    fn describe(line: &str) -> String {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let cmd = parse(line).unwrap();
        let run = async {
            // kube's client spawns its request buffer, so it needs the runtime.
            let client = Client::new(tower::service_fn(api), "demo");
            describe_cmd(&client, &cmd, &cmd.words[1..], "demo").await
        };
        match rt.block_on(run) {
            Ok(s) => s,
            Err(Fail::Kube(e)) => format!("{:?}", kube_error(e)),
            Err(Fail::Tool(e)) => format!("{e:?}"),
        }
    }

    #[test]
    fn like_kubectl() {
        let pod = str_at(&pods()[0], "/metadata/name").to_string();
        // One object: events are asked for, refused, and print as <none>.
        assert_eq!(describe(&format!("describe pod {pod}")), include_str!("testdata/describe-pod.txt"));
        assert_eq!(describe("describe deployment hello"), include_str!("testdata/describe-deployment.txt"));
        // Several: no events; a list joins them with a blank line, a prefix
        // match ends each with one.
        assert_eq!(describe("describe pods"), include_str!("testdata/describe-pods.txt"));
        assert_eq!(describe("describe pods hello"), include_str!("testdata/describe-pods-prefix.txt"));
        let missing = describe("describe pods nothere");
        assert!(missing.contains("Error from server (NotFound): pods \\\"nothere\\\" not found"), "{missing}");
    }
}

mod export {
    #![allow(unsafe_code)]
    use super::{Component, bindings};
    bindings::export!(Component with_types_in bindings);
}
