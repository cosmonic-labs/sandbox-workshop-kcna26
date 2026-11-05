//! kubectl's tables, printed here because the kube-api plugin asks the API
//! server for JSON, not for its Table format. The columns, the pod STATUS
//! logic and AGE follow kubectl's own printers (k8s.io/kubernetes
//! pkg/printers/internalversion), and the spacing follows its tabwriter.

use serde_json::Value;

use crate::{Kind, str_at};

/// Seconds since the Unix epoch, from the wall clock (the one clock WASI's
/// command world gives every component).
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// `2026-11-05T01:40:18Z` (with or without fractional seconds) as Unix
/// seconds. Kubernetes always writes UTC.
pub fn parse_time(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // Days from 1970-01-01 to the date (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) * 86400 + hh * 3600 + mm * 60 + ss)
}

/// kubectl's AGE: k8s.io/apimachinery's HumanDuration, such as 45s, 5m30s,
/// 129m, 5h12m, 2d3h or 30d.
pub fn human(secs: i64) -> String {
    if secs < -1 {
        return "<invalid>".into();
    }
    let s = secs.max(0);
    let (m, h) = (s / 60, s / 3600);
    let (d, y) = (h / 24, h / (24 * 365));
    let pair = |a: i64, ua: &str, b: i64, ub: &str| if b == 0 { format!("{a}{ua}") } else { format!("{a}{ua}{b}{ub}") };
    match () {
        _ if s < 120 => format!("{s}s"),
        _ if m < 10 => pair(m, "m", s % 60, "s"),
        _ if m < 180 => format!("{m}m"),
        _ if h < 8 => pair(h, "h", m % 60, "m"),
        _ if h < 48 => format!("{h}h"),
        _ if h < 24 * 8 => pair(d, "d", h % 24, "h"),
        _ if h < 24 * 365 * 2 => format!("{d}d"),
        _ if h < 24 * 365 * 8 => pair(y, "y", d % 365, "d"),
        _ => format!("{y}y"),
    }
}

fn age(v: &Value, ptr: &str, now: i64) -> String {
    match parse_time(str_at(v, ptr)) {
        Some(t) => human(now - t),
        None => "<unknown>".into(),
    }
}

fn arr<'a>(v: &'a Value, ptr: &str) -> &'a [Value] {
    v.pointer(ptr).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn int(v: &Value, ptr: &str) -> i64 {
    v.pointer(ptr).and_then(Value::as_i64).unwrap_or_default()
}

fn or_none(s: &str) -> String {
    if s.is_empty() { "<none>".into() } else { s.into() }
}

/// `app=hello,tier=web`: a label map as kubectl writes a selector.
fn labels(v: &Value, ptr: &str) -> String {
    v.pointer(ptr)
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default())).collect::<Vec<_>>().join(","))
        .unwrap_or_default()
}

/// A table for one resource type: the header, then a row per object, laid out
/// as kubectl's tabwriter does (minimum width 6, three spaces of padding,
/// the last column unpadded).
pub fn print(kind: &Kind, items: &[Value], all: bool, wide: bool, prefix: bool, now: i64) -> String {
    let show_ns = all && kind.namespaced;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut header: Vec<String> = headers(kind, wide).iter().map(|s| s.to_string()).collect();
    if show_ns {
        header.insert(0, "NAMESPACE".into());
    }
    rows.push(header);
    for o in items {
        let mut name = str_at(o, "/metadata/name").to_string();
        if prefix {
            name = format!("{}/{name}", kind.short());
        }
        let mut row = vec![name];
        row.extend(cells(kind, o, wide, now));
        if show_ns {
            row.insert(0, str_at(o, "/metadata/namespace").into());
        }
        rows.push(row);
    }
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols)
        .map(|i| rows.iter().filter_map(|r| r.get(i)).map(|c| c.chars().count()).max().unwrap_or(0))
        .map(|w| (w + 3).max(6))
        .collect();
    let mut out = String::new();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i + 1 == row.len() {
                out.push_str(cell);
            } else {
                out.push_str(cell);
                out.extend(std::iter::repeat_n(' ', widths[i] - cell.chars().count()));
            }
        }
        out.push('\n');
    }
    out
}

fn headers(kind: &Kind, wide: bool) -> &'static [&'static str] {
    match (kind.ar.kind.as_str(), wide) {
        ("Pod", false) => &["NAME", "READY", "STATUS", "RESTARTS", "AGE"],
        ("Pod", true) => &["NAME", "READY", "STATUS", "RESTARTS", "AGE", "IP", "NODE", "NOMINATED NODE", "READINESS GATES"],
        ("Deployment", false) => &["NAME", "READY", "UP-TO-DATE", "AVAILABLE", "AGE"],
        ("Deployment", true) => &["NAME", "READY", "UP-TO-DATE", "AVAILABLE", "AGE", "CONTAINERS", "IMAGES", "SELECTOR"],
        ("Service", false) => &["NAME", "TYPE", "CLUSTER-IP", "EXTERNAL-IP", "PORT(S)", "AGE"],
        ("Service", true) => &["NAME", "TYPE", "CLUSTER-IP", "EXTERNAL-IP", "PORT(S)", "AGE", "SELECTOR"],
        ("Secret", _) => &["NAME", "TYPE", "DATA", "AGE"],
        ("ConfigMap", _) => &["NAME", "DATA", "AGE"],
        ("Namespace", _) => &["NAME", "STATUS", "AGE"],
        // The CRD's additionalPrinterColumns (kubectl get crd
        // workloaddeployments.runtime.wasmcloud.dev -o yaml). agent-reader may
        // not read CRDs, so they are written here. No AGE: a CRD that lists
        // its own columns doesn't get one.
        ("WorkloadDeployment", _) => &["NAME", "REPLICAS", "READY"],
        _ => &["NAME"],
    }
}

/// The cells after NAME.
fn cells(kind: &Kind, o: &Value, wide: bool, now: i64) -> Vec<String> {
    let created = age(o, "/metadata/creationTimestamp", now);
    match kind.ar.kind.as_str() {
        "Pod" => pod(o, wide, created, now),
        "Deployment" => {
            let mut c = vec![
                format!("{}/{}", int(o, "/status/readyReplicas"), int(o, "/spec/replicas")),
                int(o, "/status/updatedReplicas").to_string(),
                int(o, "/status/availableReplicas").to_string(),
                created,
            ];
            if wide {
                let containers = arr(o, "/spec/template/spec/containers");
                let join = |p: &str| containers.iter().map(|c| str_at(c, p)).collect::<Vec<_>>().join(",");
                c.extend([join("/name"), join("/image"), or_none(&labels(o, "/spec/selector/matchLabels"))]);
            }
            c
        }
        "Service" => {
            let mut c = vec![
                str_at(o, "/spec/type").to_string(),
                or_none(str_at(o, "/spec/clusterIP")),
                external_ip(o),
                ports(o),
                created,
            ];
            if wide {
                c.push(or_none(&labels(o, "/spec/selector")));
            }
            c
        }
        "Secret" => {
            let data = o.pointer("/data").and_then(Value::as_object).map_or(0, |m| m.len());
            vec![str_at(o, "/type").to_string(), data.to_string(), created]
        }
        "ConfigMap" => {
            let n = |p: &str| o.pointer(p).and_then(Value::as_object).map_or(0, |m| m.len());
            vec![(n("/data") + n("/binaryData")).to_string(), created]
        }
        "Namespace" => vec![str_at(o, "/status/phase").to_string(), created],
        "WorkloadDeployment" => {
            let ready = arr(o, "/status/conditions")
                .iter()
                .find(|c| str_at(c, "/type") == "Ready")
                .map(|c| str_at(c, "/status"))
                .unwrap_or_default();
            let replicas = o.pointer("/spec/replicas").map(|v| v.to_string()).unwrap_or_default();
            vec![replicas, ready.to_string()]
        }
        _ => vec![],
    }
}

/// A pod's READY, STATUS, RESTARTS and AGE (and the wide columns), with
/// kubectl's status logic: a waiting or terminated container's reason wins
/// over the phase, an init container still running shows as Init:N/M, and a
/// pod being deleted shows as Terminating.
fn pod(o: &Value, wide: bool, created: String, now: i64) -> Vec<String> {
    let mut reason = match str_at(o, "/status/reason") {
        "" => str_at(o, "/status/phase").to_string(),
        r => r.to_string(),
    };
    let containers = arr(o, "/spec/containers");
    let init_specs = arr(o, "/spec/initContainers");
    // Sidecars (init containers that restart) count as containers.
    let sidecar = |name: &str| {
        init_specs.iter().any(|c| str_at(c, "/name") == name && str_at(c, "/restartPolicy") == "Always")
    };
    let total = containers.len() + init_specs.iter().filter(|c| str_at(c, "/restartPolicy") == "Always").count();
    // RESTARTS: the count, and when the latest restart was.
    let mut ready = 0;
    let note = |c: &Value, (n, last): &mut (i64, Option<i64>)| {
        *n += int(c, "/restartCount");
        if let Some(t) = parse_time(str_at(c, "/lastState/terminated/finishedAt")) {
            *last = Some(last.map_or(t, |l| l.max(t)));
        }
    };
    let (mut restarts, mut sidecar_restarts) = ((0, None), (0, None));

    let mut initializing = false;
    let init = arr(o, "/status/initContainerStatuses");
    for (i, c) in init.iter().enumerate() {
        note(c, &mut restarts);
        if sidecar(str_at(c, "/name")) {
            note(c, &mut sidecar_restarts);
        }
        let term = c.pointer("/state/terminated");
        let waiting = str_at(c, "/state/waiting/reason");
        if term.is_some() && int(c, "/state/terminated/exitCode") == 0 {
            continue;
        }
        if sidecar(str_at(c, "/name")) && c.pointer("/started") == Some(&Value::Bool(true)) {
            if c.pointer("/ready") == Some(&Value::Bool(true)) {
                ready += 1;
            }
            continue;
        }
        reason = if let Some(t) = term {
            match str_at(t, "/reason") {
                "" if int(t, "/signal") != 0 => format!("Init:Signal:{}", int(t, "/signal")),
                "" => format!("Init:ExitCode:{}", int(t, "/exitCode")),
                r => format!("Init:{r}"),
            }
        } else if !waiting.is_empty() && waiting != "PodInitializing" {
            format!("Init:{waiting}")
        } else {
            format!("Init:{i}/{}", init_specs.len())
        };
        initializing = true;
        break;
    }
    let initialized = arr(o, "/status/conditions")
        .iter()
        .any(|c| str_at(c, "/type") == "Initialized" && str_at(c, "/status") == "True");
    if !initializing || initialized {
        // Init containers that finished no longer count, sidecars still do.
        restarts = sidecar_restarts;
        let mut has_running = false;
        for c in arr(o, "/status/containerStatuses").iter().rev() {
            note(c, &mut restarts);
            let waiting = str_at(c, "/state/waiting/reason");
            let term = c.pointer("/state/terminated");
            if !waiting.is_empty() {
                reason = waiting.into();
            } else if let Some(t) = term {
                reason = match str_at(t, "/reason") {
                    "" if int(t, "/signal") != 0 => format!("Signal:{}", int(t, "/signal")),
                    "" => format!("ExitCode:{}", int(t, "/exitCode")),
                    r => r.into(),
                };
            } else if c.pointer("/ready") == Some(&Value::Bool(true)) && c.pointer("/state/running").is_some() {
                has_running = true;
                ready += 1;
            }
        }
        // A pod whose containers finished but one still runs is Running.
        if reason == "Completed" && has_running {
            let pod_ready = arr(o, "/status/conditions")
                .iter()
                .any(|c| str_at(c, "/type") == "Ready" && str_at(c, "/status") == "True");
            reason = if pod_ready { "Running" } else { "NotReady" }.into();
        }
    }
    if o.pointer("/metadata/deletionTimestamp").is_some() {
        let phase = str_at(o, "/status/phase");
        if str_at(o, "/status/reason") == "NodeLost" {
            reason = "Unknown".into();
        } else if phase != "Succeeded" && phase != "Failed" {
            reason = "Terminating".into();
        }
    }
    let restarts = match restarts {
        (n, Some(t)) if n > 0 => format!("{n} ({} ago)", human(now - t)),
        (n, _) => n.to_string(),
    };
    let mut c = vec![format!("{ready}/{total}"), reason, restarts, created];
    if wide {
        let gates = arr(o, "/spec/readinessGates");
        let gates = if gates.is_empty() {
            "<none>".to_string()
        } else {
            let conds = arr(o, "/status/conditions");
            let met = gates
                .iter()
                .filter(|g| {
                    let t = str_at(g, "/conditionType");
                    conds.iter().any(|c| str_at(c, "/type") == t && str_at(c, "/status") == "True")
                })
                .count();
            format!("{met}/{}", gates.len())
        };
        c.extend([
            or_none(str_at(o, "/status/podIP")),
            or_none(str_at(o, "/spec/nodeName")),
            or_none(str_at(o, "/status/nominatedNodeName")),
            gates,
        ]);
    }
    c
}

/// EXTERNAL-IP, as kubectl works it out for each service type.
fn external_ip(o: &Value) -> String {
    let external: Vec<&str> = arr(o, "/spec/externalIPs").iter().filter_map(Value::as_str).collect();
    let joined = |extra: Vec<&str>| {
        let all: Vec<&str> = extra.into_iter().chain(external.iter().copied()).collect();
        if all.is_empty() { "<none>".to_string() } else { all.join(",") }
    };
    match str_at(o, "/spec/type") {
        "ClusterIP" | "NodePort" => joined(vec![]),
        "LoadBalancer" => {
            let lb: Vec<&str> = arr(o, "/status/loadBalancer/ingress")
                .iter()
                .map(|i| match str_at(i, "/ip") {
                    "" => str_at(i, "/hostname"),
                    ip => ip,
                })
                .collect();
            if lb.is_empty() && external.is_empty() { "<pending>".into() } else { joined(lb) }
        }
        "ExternalName" => str_at(o, "/spec/externalName").into(),
        _ => "<unknown>".into(),
    }
}

/// PORT(S): `80/TCP`, or `80:30180/TCP` with a node port.
fn ports(o: &Value) -> String {
    let p: Vec<String> = arr(o, "/spec/ports")
        .iter()
        .map(|p| {
            let proto = str_at(p, "/protocol");
            match int(p, "/nodePort") {
                0 => format!("{}/{proto}", int(p, "/port")),
                n => format!("{}:{n}/{proto}", int(p, "/port")),
            }
        })
        .collect();
    if p.is_empty() { "<none>".into() } else { p.join(",") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_match_kubectl() {
        for (s, want) in [
            (45, "45s"), (119, "119s"), (150, "2m30s"), (180, "3m"), (7740, "129m"),
            (18720, "5h12m"), (28800, "8h"), (183600, "2d3h"), (2592000, "30d"),
        ] {
            assert_eq!(human(s), want, "{s}");
        }
    }

    #[test]
    fn times_parse() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-05T01:40:18Z"), Some(1793842818));
        assert_eq!(parse_time("2026-11-05T01:40:18.123456Z"), Some(1793842818));
    }
}
