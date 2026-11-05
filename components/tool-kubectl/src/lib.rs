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

mod bindings {
    #![allow(unsafe_code)]
    wit_bindgen::generate!({ path: "../../wit", world: "workshop:toolbox/kubectl-tool", generate_all });
}

use std::convert::Infallible;
use std::fmt::Write as _;

use bindings::exports::workshop::toolbox::kubectl::{Error, Guest, RunArgs};
use bindings::workshop::toolbox::kube_api as api;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, Secret, Service};
use kube::api::{Api, ApiResource, DynamicObject, ListParams, LogParams};
use kube::Client;

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

const UNSUPPORTED: &[&str] = &["exec", "port-forward", "attach", "cp", "proxy", "run", "debug", "auth", "config"];

async fn run(cmd: &str) -> Result<String, Error> {
    let args: Vec<&str> = cmd.split_whitespace().collect();
    let Some((&verb, rest)) = args.split_first() else {
        return Err(Error::InvalidArgument("usage: get <resource> [name] [-n ns | -A] | logs <pod>|deployment/<name> [-n ns]".into()));
    };
    if UNSUPPORTED.contains(&verb) {
        return Err(Error::AccessDenied(format!(
            "\"{verb}\" is not available: this kubectl sends plain API requests through the kube-api capability, which has no streaming, upgrade or local credential"
        )));
    }
    // Like a kubeconfig context's namespace: the one this workload's
    // service account may read (k8s/kube.yaml).
    let mut ns = "demo".to_string();
    let mut all = false;
    let mut pos = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "-n" | "--namespace" => ns = it.next().map(|s| s.to_string()).unwrap_or_default(),
            "-A" | "--all-namespaces" => all = true,
            other => pos.push(other),
        }
    }
    // `raw METHOD PATH`: send exactly this request through the capability, to
    // show what the capability itself allows, past this tool's own checks.
    if verb == "raw" {
        let (Some(method), Some(path)) = (pos.first(), pos.get(1)) else {
            return Err(Error::InvalidArgument("usage: raw <METHOD> <path>".into()));
        };
        return match wit_bindgen::block_on(api::send(method.to_string(), path.to_string(), Vec::new())) {
            Ok(r) => Ok(format!("HTTP {}\n{}\n", r.status, String::from_utf8_lossy(&r.body))),
            Err(e) => Err(Error::Network(e)),
        };
    }
    let client = Client::new(tower::service_fn(via_capability), ns.clone());
    let result = match verb {
        "get" => get(client, &pos, &ns, all).await,
        "logs" => match pos.first() {
            Some(target) => logs(client, target, &ns).await,
            None => return Err(Error::InvalidArgument("logs needs a pod name or deployment/<name>".into())),
        },
        other => return Err(Error::InvalidArgument(format!("unknown command \"{other}\""))),
    };
    match result {
        Ok(s) => Ok(s),
        // 502 is this tool's own marker for "the capability could not send it".
        Err(kube::Error::Api(s)) if s.code == 502 => Err(Error::Network(s.message)),
        Err(kube::Error::Api(s)) if s.code == 403 || s.code == 401 => {
            Err(Error::AccessDenied(format!("Error from server ({}): {}", s.reason, s.message)))
        }
        Err(kube::Error::Api(s)) => Err(Error::Other(Some(format!("Error from server ({}): {}", s.reason, s.message)))),
        Err(e) => Err(Error::Other(Some(e.to_string()))),
    }
}

/// `logs <pod>`, or `logs deployment/<name>`, which reads the first of the
/// deployment's pods by name, as kubectl picks one pod for you.
async fn logs(client: Client, target: &str, ns: &str) -> kube::Result<String> {
    let pods = Api::<Pod>::namespaced(client.clone(), ns);
    let Some(name) = target.strip_prefix("deployment/").or_else(|| target.strip_prefix("deploy/")) else {
        return pods.logs(target, &LogParams::default()).await;
    };
    // Read as dynamic objects: typed Deployment and Pod lists would pull
    // their whole deserializers into the component.
    let ar = ApiResource::erase::<Deployment>(&());
    let deploy = Api::<DynamicObject>::namespaced_with(client.clone(), ns, &ar).get(name).await?;
    let selector = deploy
        .data
        .pointer("/spec/selector/matchLabels")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let pod_ar = ApiResource::erase::<Pod>(&());
    let mut names: Vec<String> = Api::<DynamicObject>::namespaced_with(client, ns, &pod_ar)
        .list(&ListParams::default().labels(&selector))
        .await?
        .items
        .into_iter()
        .filter_map(|p| p.metadata.name)
        .collect();
    names.sort();
    match names.first() {
        Some(pod) => pods.logs(pod, &LogParams::default()).await,
        None => Ok(format!("error: deployment/{name} has no pods\n")),
    }
}

fn resource(name: &str) -> Option<ApiResource> {
    Some(match name {
        "pods" | "pod" | "po" => ApiResource::erase::<Pod>(&()),
        "deployments" | "deployment" | "deploy" => ApiResource::erase::<Deployment>(&()),
        "services" | "service" | "svc" => ApiResource::erase::<Service>(&()),
        "secrets" | "secret" => ApiResource::erase::<Secret>(&()),
        "configmaps" | "configmap" | "cm" => ApiResource::erase::<ConfigMap>(&()),
        "namespaces" | "namespace" | "ns" => ApiResource::erase::<Namespace>(&()),
        "workloaddeployments" | "workloaddeployment" => ApiResource {
            group: "runtime.wasmcloud.dev".into(),
            version: "v1alpha1".into(),
            api_version: "runtime.wasmcloud.dev/v1alpha1".into(),
            kind: "WorkloadDeployment".into(),
            plural: "workloaddeployments".into(),
        },
        _ => return None,
    })
}

async fn get(client: Client, pos: &[&str], ns: &str, all: bool) -> kube::Result<String> {
    let Some(kind) = pos.first() else {
        return Ok("error: get needs a resource type\n".into());
    };
    let Some(ar) = resource(kind) else {
        return Ok(format!("error: this kubectl doesn't know the resource type \"{kind}\" (it knows pods, deployments, services, secrets, configmaps, namespaces, workloaddeployments)\n"));
    };
    let cluster_scoped = ar.kind == "Namespace";
    let api: Api<DynamicObject> = if all || cluster_scoped {
        Api::all_with(client, &ar)
    } else {
        Api::namespaced_with(client, ns, &ar)
    };
    if let Some(name) = pos.get(1) {
        let obj = api.get(name).await?;
        return Ok(serde_json::to_string_pretty(&obj).unwrap_or_default() + "\n");
    }
    let list = api.list(&ListParams::default()).await?;
    if list.items.is_empty() {
        return Ok(format!("No resources found in {ns} namespace.\n"));
    }
    let mut out = String::new();
    let show_ns = all && !cluster_scoped;
    if show_ns {
        let _ = writeln!(out, "{:<20} {:<44} {}", "NAMESPACE", "NAME", "STATUS");
    } else {
        let _ = writeln!(out, "{:<44} {}", "NAME", "STATUS");
    }
    for o in list.items {
        let name = o.metadata.name.clone().unwrap_or_default();
        // A pod has a phase; a deployment has ready and wanted replicas.
        let status = match o.data.pointer("/status/phase").and_then(|v| v.as_str()) {
            Some(phase) => phase.to_string(),
            None => match o.data.pointer("/spec/replicas").and_then(|v| v.as_u64()) {
                Some(want) => {
                    let ready = o.data.pointer("/status/readyReplicas").and_then(|v| v.as_u64()).unwrap_or(0);
                    format!("{ready}/{want} ready")
                }
                None => String::new(),
            },
        };
        if show_ns {
            let _ = writeln!(out, "{:<20} {:<44} {}", o.metadata.namespace.unwrap_or_default(), name, status);
        } else {
            let _ = writeln!(out, "{:<44} {}", name, status);
        }
    }
    Ok(out)
}

mod export {
    #![allow(unsafe_code)]
    use super::{Component, bindings};
    bindings::export!(Component with_types_in bindings);
}
