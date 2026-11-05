//! kubectl's describers, over the JSON the API server returns. Each function
//! follows its namesake in k8s.io/kubectl pkg/describe (v1.37): the same
//! fields in the same order, the same wording, and the same layout, which is
//! Go's text/tabwriter (minwidth 0, padding 2, spaces). Times are printed in
//! UTC, as kubectl prints them with TZ=UTC: a component has no time zone.
//!
//! The objects are serde_json Values, not k8s-openapi's typed structs, whose
//! deserializers would add a large share of the component's size.

use serde_json::{Map, Value};

use crate::str_at;
use crate::table::{human, parse_time};

/// What a describer may need besides the object. `None` means the request was
/// not made or failed, which kubectl treats as "leave that section out".
#[derive(Default)]
pub struct Extra {
    pub events: Option<Vec<Value>>,
    /// A deployment's replica sets, old and new.
    pub replica_sets: Option<(Vec<Value>, Vec<Value>)>,
    pub endpoint_slices: Vec<Value>,
    /// A namespace's resource quotas and limit ranges.
    pub quotas: Option<Vec<Value>>,
    pub limit_ranges: Option<Vec<Value>>,
}

/// Describe one object. `kind` is its Kind; any kind without a describer of
/// its own gets kubectl's generic (unstructured) describer.
pub fn describe(kind: &str, o: &Value, x: &Extra, now: i64) -> String {
    let mut w = W::default();
    match kind {
        "Pod" => pod(&mut w, o, x, now),
        "Deployment" => deployment(&mut w, o, x, now),
        "Service" => service(&mut w, o, x, now),
        "Secret" => secret(&mut w, o),
        "ConfigMap" => config_map(&mut w, o, x, now),
        "Namespace" => namespace(&mut w, o, x),
        _ => generic(&mut w, o, x, now),
    }
    // The terminal escapes kubectl applies to everything it writes.
    tabwriter(&w.0.replace('\x1b', "^[").replace('\r', "\\r"))
}

// ---------------------------------------------------------------------------
// Writing: kubectl's PrefixWriter and Go's tabwriter.
// ---------------------------------------------------------------------------

/// kubectl's PrefixWriter: two spaces per level, then the text.
#[derive(Default)]
struct W(String);

impl W {
    fn s(&mut self, level: usize, s: &str) {
        self.indent(level);
        self.0.push_str(s);
    }
    fn indent(&mut self, level: usize) {
        for _ in 0..level {
            self.0.push_str("  ");
        }
    }
}

/// `out!(w, level, "fmt", args...)`: PrefixWriter.Write, formatting straight
/// into the buffer.
macro_rules! out {
    ($w:expr, $level:expr, $($arg:tt)*) => {{
        $w.indent($level);
        let _ = std::fmt::Write::write_fmt(&mut $w.0, format_args!($($arg)*));
    }};
}

/// Go's text/tabwriter as kubectl configures it: a cell ends at a tab, a
/// column block is a run of lines that have a cell in that column, and each
/// cell is padded to the block's widest cell plus two. The text after a
/// line's last tab is not a cell and is never padded.
pub fn tabwriter(text: &str) -> String {
    let lines: Vec<Vec<&str>> = text.split('\n').map(|l| l.split('\t').collect()).collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut widths = Vec::new();
    format(&lines, &mut widths, 0, lines.len(), &mut out);
    out
}

fn format(lines: &[Vec<&str>], widths: &mut Vec<usize>, mut line0: usize, line1: usize, out: &mut String) {
    let column = widths.len();
    let mut this = line0;
    while this < line1 {
        if column + 1 >= lines[this].len() {
            this += 1;
            continue;
        }
        write_lines(lines, widths, line0, this, out);
        line0 = this;
        let mut width = 0;
        while this < line1 && column + 1 < lines[this].len() {
            width = width.max(lines[this][column].chars().count() + 2);
            this += 1;
        }
        widths.push(width);
        format(lines, widths, line0, this, out);
        widths.pop();
        line0 = this;
    }
    write_lines(lines, widths, line0, line1, out);
}

fn write_lines(lines: &[Vec<&str>], widths: &[usize], line0: usize, line1: usize, out: &mut String) {
    for (i, line) in lines.iter().enumerate().take(line1).skip(line0) {
        for (j, cell) in line.iter().enumerate() {
            out.push_str(cell);
            if j < widths.len() {
                out.extend(std::iter::repeat_n(' ', widths[j].saturating_sub(cell.chars().count())));
            }
        }
        if i + 1 < lines.len() {
            out.push('\n');
        }
    }
}

// ---------------------------------------------------------------------------
// Values.
// ---------------------------------------------------------------------------

fn arr<'a>(v: &'a Value, ptr: &str) -> &'a [Value] {
    v.pointer(ptr).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn obj<'a>(v: &'a Value, ptr: &str) -> Option<&'a Map<String, Value>> {
    v.pointer(ptr).and_then(Value::as_object).filter(|m| !m.is_empty())
}

fn int(v: &Value, ptr: &str) -> i64 {
    v.pointer(ptr).and_then(Value::as_i64).unwrap_or_default()
}

fn flag(v: &Value, ptr: &str) -> bool {
    v.pointer(ptr).and_then(Value::as_bool).unwrap_or_default()
}

/// A map's entries as strings, sorted by key.
fn string_map(v: &Value, ptr: &str) -> Vec<(String, String)> {
    let Some(m) = obj(v, ptr) else { return Vec::new() };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    order(&keys).into_iter().map(|i| (keys[i].to_string(), go_v(&m[keys[i]]))).collect()
}

/// The objects of a list in order of the string at `ptr`, ties kept in list
/// order. (One sort over (key, index) pairs keeps the component small: each
/// sort closure would add its own copy of the sort.)
fn sorted_by<'a>(list: &'a [Value], ptr: &str) -> Vec<&'a Value> {
    let keys: Vec<&str> = list.iter().map(|v| str_at(v, ptr)).collect();
    order(&keys).into_iter().map(|i| &list[i]).collect()
}

/// The indexes of `keys` in sorted order, ties in their original order.
fn order(keys: &[&str]) -> Vec<usize> {
    let mut keyed: Vec<(&str, usize)> = keys.iter().copied().zip(0..).collect();
    keyed.sort_unstable();
    keyed.into_iter().map(|(_, i)| i).collect()
}

fn sort_strs(v: &mut Vec<&str>) {
    *v = order(v).into_iter().map(|i| v[i]).collect();
}

/// A JSON value as Go's `%v` prints it once decoded into interface{}.
fn go_v(v: &Value) -> String {
    match v {
        Value::Null => "<nil>".into(),
        Value::String(s) => s.clone(),
        Value::Array(a) => format!("[{}]", a.iter().map(go_v).collect::<Vec<_>>().join(" ")),
        Value::Object(m) => {
            let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
            sort_strs(&mut keys);
            format!("map[{}]", keys.iter().map(|k| format!("{k}:{}", go_v(&m[*k]))).collect::<Vec<_>>().join(" "))
        }
        v => v.to_string(),
    }
}

fn or_none(s: &str) -> String {
    if s.is_empty() { "<none>".into() } else { s.into() }
}

/// `Thu, 05 Nov 2026 01:40:29 +0000`: Go's time.RFC1123Z, in UTC. A missing
/// time prints as Go's zero time, as kubectl's does.
pub fn rfc1123z(ts: &str) -> String {
    let Some(t) = parse_time(ts) else {
        return "Mon, 01 Jan 0001 00:00:00 +0000".into();
    };
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!(
        "{}, {d:02} {} {y:04} {:02}:{:02}:{:02} +0000",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(m - 1) as usize],
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// The decoded length of base64 text, as kubectl counts a Secret's bytes.
fn b64_len(s: &str) -> usize {
    let s = s.trim_end_matches('=');
    s.len() * 3 / 4
}

fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for v in s.bytes().filter_map(val) {
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// A label selector as kubectl prints one: requirements sorted by key,
/// `k=v`, `k in (a,b)`, `k notin (a,b)`, `k` or `!k`.
pub fn selector(sel: &Value) -> String {
    let mut reqs: Vec<(String, String)> = string_map(sel, "/matchLabels")
        .into_iter()
        .map(|(k, v)| (k.clone(), format!("{k}={v}")))
        .collect();
    for e in arr(sel, "/matchExpressions") {
        let key = str_at(e, "/key");
        let mut values: Vec<&str> = arr(e, "/values").iter().filter_map(Value::as_str).collect();
        sort_strs(&mut values);
        let r = match str_at(e, "/operator") {
            "In" => format!("{key} in ({})", values.join(",")),
            "NotIn" => format!("{key} notin ({})", values.join(",")),
            "Exists" => key.to_string(),
            "DoesNotExist" => format!("!{key}"),
            _ => continue,
        };
        reqs.push((key.to_string(), r));
    }
    let keys: Vec<&str> = reqs.iter().map(|(k, _)| k.as_str()).collect();
    order(&keys).into_iter().map(|i| reqs[i].1.as_str()).collect::<Vec<_>>().join(",")
}

/// labels.FormatLabels: `k=v,k2=v2`, or `<none>`.
fn format_labels(v: &Value, ptr: &str) -> String {
    let kv = string_map(v, ptr);
    if kv.is_empty() {
        return "<none>".into();
    }
    kv.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")
}

// ---------------------------------------------------------------------------
// Shared sections.
// ---------------------------------------------------------------------------

fn labels_multiline(w: &mut W, title: &str, v: &Value, ptr: &str) {
    out!(w, 0, "{title}:\t");
    let kv = string_map(v, ptr);
    if kv.is_empty() {
        w.s(0, "<none>\n");
    }
    for (i, (k, v)) in kv.iter().enumerate() {
        if i != 0 {
            w.s(0, "\t");
        }
        out!(w, 0, "{k}={v}\n");
    }
}

const MAX_ANNOTATION_LEN: usize = 140;

fn annotations_multiline(w: &mut W, title: &str, v: &Value, ptr: &str) {
    out!(w, 0, "{title}:\t");
    let kv: Vec<_> = string_map(v, ptr)
        .into_iter()
        .filter(|(k, _)| k != "kubectl.kubernetes.io/last-applied-configuration")
        .collect();
    if kv.is_empty() {
        w.s(0, "<none>\n");
        return;
    }
    for (i, (k, value)) in kv.iter().enumerate() {
        if i != 0 {
            w.s(0, "\t");
        }
        let value = value.strip_suffix('\n').unwrap_or(value);
        if value.len() + k.len() + 2 > MAX_ANNOTATION_LEN || value.contains('\n') {
            out!(w, 0, "{k}:\n");
            for s in value.split('\n') {
                out!(w, 0, "\t  {}\n", shorten(s, MAX_ANNOTATION_LEN - 2));
            }
        } else {
            out!(w, 0, "{k}: {value}\n");
        }
    }
}

fn shorten(s: &str, max: usize) -> String {
    if s.len() > max {
        // Go slices bytes; stay on a character boundary.
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    } else {
        s.into()
    }
}

fn tolerations_multiline(w: &mut W, title: &str, v: &Value, ptr: &str) {
    out!(w, 0, "{title}:\t");
    let tols = sorted_by(arr(v, ptr), "/key");
    if tols.is_empty() {
        w.s(0, "<none>\n");
        return;
    }
    let indent = title.len() - title.trim_start().len();
    for (i, t) in tols.iter().enumerate() {
        if i != 0 {
            w.s(0, &" ".repeat(indent));
            w.s(0, "\t");
        }
        let (key, value, effect) = (str_at(t, "/key"), str_at(t, "/value"), str_at(t, "/effect"));
        let mut s = key.to_string();
        if !value.is_empty() {
            s.push_str(&format!("={value}"));
        }
        if !effect.is_empty() {
            s.push_str(&format!(":{effect}"));
        }
        if str_at(t, "/operator") == "Exists" && value.is_empty() {
            s.push_str(if key.is_empty() && effect.is_empty() { "op=Exists" } else { " op=Exists" });
        }
        if let Some(secs) = t.pointer("/tolerationSeconds").and_then(Value::as_i64) {
            s.push_str(&format!(" for {secs}s"));
        }
        out!(w, 0, "{s}\n");
    }
}

/// DescribeEvents. Events are sorted by their last timestamp.
fn events(w: &mut W, x: &Extra, now: i64) {
    let Some(events) = &x.events else { return };
    if events.is_empty() {
        w.s(0, "Events:\t<none>\n");
        return;
    }
    // RFC 3339 times in UTC sort as text; a missing one first, as Go's zero time.
    let events = sorted_by(events, "/lastTimestamp");
    w.s(0, "Events:\n  Type\tReason\tAge\tFrom\tMessage\n");
    w.s(1, "----\t------\t----\t----\t-------\n");
    let since = |ts: &str| parse_time(ts).map_or("<unknown>".to_string(), |t| human(now - t));
    for e in events {
        let first = match str_at(e, "/eventTime") {
            "" => since(str_at(e, "/firstTimestamp")),
            t => since(t),
        };
        let interval = if e.pointer("/series").is_some_and(|s| !s.is_null()) {
            format!("{} (x{} over {first})", since(str_at(e, "/series/lastObservedTime")), int(e, "/series/count"))
        } else if int(e, "/count") > 1 {
            format!("{} (x{} over {first})", since(str_at(e, "/lastTimestamp")), int(e, "/count"))
        } else {
            first
        };
        let source = match str_at(e, "/source/component") {
            "" => str_at(e, "/reportingComponent"),
            s => s,
        };
        let mut message = str_at(e, "/message").trim().to_string();
        let field = str_at(e, "/involvedObject/fieldPath");
        if !field.is_empty() {
            message = format!("{field}: {message}");
        }
        out!(w, 1, "{}\t{}\t{interval}\t{source}\t{message}\n", str_at(e, "/type"), str_at(e, "/reason"));
    }
}

fn controlled_by(o: &Value) -> Option<String> {
    arr(o, "/metadata/ownerReferences")
        .iter()
        .find(|r| flag(r, "/controller"))
        .map(|r| format!("{}/{}", str_at(r, "/kind"), str_at(r, "/name")))
}

// ---------------------------------------------------------------------------
// Pods and their containers.
// ---------------------------------------------------------------------------

fn pod(w: &mut W, o: &Value, x: &Extra, now: i64) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    out!(w, 0, "Namespace:\t{}\n", str_at(o, "/metadata/namespace"));
    if let Some(p) = o.pointer("/spec/priority").and_then(Value::as_i64) {
        out!(w, 0, "Priority:\t{p}\n");
    }
    let pc = str_at(o, "/spec/priorityClassName");
    if !pc.is_empty() {
        out!(w, 0, "Priority Class Name:\t{pc}\n");
    }
    let rc = str_at(o, "/spec/runtimeClassName");
    if !rc.is_empty() {
        out!(w, 0, "Runtime Class Name:\t{rc}\n");
    }
    let sa = str_at(o, "/spec/serviceAccountName");
    if !sa.is_empty() {
        out!(w, 0, "Service Account:\t{sa}\n");
    }
    match str_at(o, "/spec/nodeName") {
        "" => w.s(0, "Node:\t<none>\n"),
        n => out!(w, 0, "Node:\t{n}/{}\n", str_at(o, "/status/hostIP")),
    }
    let start = str_at(o, "/status/startTime");
    if !start.is_empty() {
        out!(w, 0, "Start Time:\t{}\n", rfc1123z(start));
    }
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    let phase = str_at(o, "/status/phase");
    let deleting = str_at(o, "/metadata/deletionTimestamp");
    if !deleting.is_empty() && phase != "Failed" && phase != "Succeeded" {
        let lasts = parse_time(deleting).map_or("<unknown>".into(), |t| human(now - t));
        out!(w, 0, "Status:\tTerminating (lasts {lasts})\n");
        out!(w, 0, "Termination Grace Period:\t{}s\n", int(o, "/metadata/deletionGracePeriodSeconds"));
    } else {
        out!(w, 0, "Status:\t{phase}\n");
    }
    for (label, ptr) in [("Reason", "/status/reason"), ("Message", "/status/message")] {
        let s = str_at(o, ptr);
        if !s.is_empty() {
            out!(w, 0, "{label}:\t{s}\n");
        }
    }
    let seccomp = str_at(o, "/spec/securityContext/seccompProfile/type");
    if !seccomp.is_empty() {
        out!(w, 0, "SeccompProfile:\t{seccomp}\n");
        if seccomp == "Localhost" {
            out!(w, 0, "LocalhostProfile:\t{}\n", str_at(o, "/spec/securityContext/seccompProfile/localhostProfile"));
        }
    }
    out!(w, 0, "IP:\t{}\n", str_at(o, "/status/podIP"));
    let ips = arr(o, "/status/podIPs");
    if ips.is_empty() {
        w.s(0, "IPs:\t<none>\n");
    } else {
        w.s(0, "IPs:\n");
        for ip in ips {
            out!(w, 1, "IP:\t{}\n", str_at(ip, "/ip"));
        }
    }
    if let Some(c) = controlled_by(o) {
        out!(w, 0, "Controlled By:\t{c}\n");
    }
    let nominated = str_at(o, "/status/nominatedNodeName");
    if !nominated.is_empty() {
        out!(w, 0, "NominatedNodeName:\t{nominated}\n");
    }
    if let Some(r) = o.pointer("/spec/resources") {
        w.s(0, "Resources:\n");
        resources(w, r, 1);
    }
    let resolve = |e: &Value| env_field(o, e);
    if !arr(o, "/spec/initContainers").is_empty() {
        containers(w, "Init Containers", arr(o, "/spec/initContainers"), Some(arr(o, "/status/initContainerStatuses")), Some(&resolve), "");
    }
    containers(w, "Containers", arr(o, "/spec/containers"), Some(arr(o, "/status/containerStatuses")), Some(&resolve), "");
    if !arr(o, "/spec/ephemeralContainers").is_empty() {
        containers(
            w,
            "Ephemeral Containers",
            arr(o, "/spec/ephemeralContainers"),
            Some(arr(o, "/status/ephemeralContainerStatuses")),
            Some(&resolve),
            "",
        );
    }
    let conditions = arr(o, "/status/conditions");
    let gates = arr(o, "/spec/readinessGates");
    if !gates.is_empty() {
        w.s(0, "Readiness Gates:\n  Type\tStatus\n");
        for g in gates {
            let t = str_at(g, "/conditionType");
            let status = conditions
                .iter()
                .find(|c| str_at(c, "/type") == t)
                .map_or("<none>", |c| str_at(c, "/status"));
            out!(w, 1, "{t} \t{status} \n");
        }
    }
    if !conditions.is_empty() {
        w.s(0, "Conditions:\n  Type\tStatus\n");
        for c in conditions {
            out!(w, 1, "{} \t{} \n", str_at(c, "/type"), str_at(c, "/status"));
        }
    }
    volumes(w, arr(o, "/spec/volumes"), "");
    out!(w, 0, "QoS Class:\t{}\n", qos(o));
    labels_multiline(w, "Node-Selectors", o, "/spec/nodeSelector");
    tolerations_multiline(w, "Tolerations", o, "/spec/tolerations");
    topology_spread(w, arr(o, "/spec/topologySpreadConstraints"), "");
    if let Some(g) = o.pointer("/spec/schedulingGroup").filter(|g| !g.is_null()) {
        w.s(0, "SchedulingGroup:\n");
        out!(w, 1, "PodGroupName:\t{}\n", str_at(g, "/podGroupName"));
    }
    events(w, x, now);
}

/// The pod's QoS class: the one the API server recorded, else worked out as
/// kubectl does.
fn qos(o: &Value) -> String {
    let recorded = str_at(o, "/status/qosClass");
    if !recorded.is_empty() {
        return recorded.into();
    }
    let all: Vec<&Value> = arr(o, "/spec/containers").iter().chain(arr(o, "/spec/initContainers")).collect();
    let mut any = false;
    let mut guaranteed = true;
    for c in &all {
        let req = obj(c, "/resources/requests");
        let lim = obj(c, "/resources/limits");
        let cm = |m: Option<&Map<String, Value>>, k: &str| m.and_then(|m| m.get(k)).cloned();
        for k in ["cpu", "memory"] {
            if cm(req, k).is_some() || cm(lim, k).is_some() {
                any = true;
            }
            match (cm(req, k), cm(lim, k)) {
                (_, None) => guaranteed = false,
                (Some(r), Some(l)) if r != l => guaranteed = false,
                _ => {}
            }
        }
    }
    match (any, guaranteed) {
        (false, _) => "BestEffort",
        (true, true) => "Guaranteed",
        _ => "Burstable",
    }
    .into()
}

/// The value kubectl shows for an env var set from a pod field: only the
/// metadata fields resolve (k8s.io/kubectl fieldpath), anything else is "".
fn env_field(pod: &Value, e: &Value) -> String {
    let path = str_at(e, "/valueFrom/fieldRef/fieldPath");
    let sub = |prefix: &str| {
        path.strip_prefix(prefix)
            .and_then(|r| r.strip_prefix("['"))
            .and_then(|r| r.strip_suffix("']"))
    };
    if let Some(k) = sub("metadata.labels") {
        return str_at(pod, &format!("/metadata/labels/{}", k.replace('~', "~0").replace('/', "~1"))).into();
    }
    if let Some(k) = sub("metadata.annotations") {
        return str_at(pod, &format!("/metadata/annotations/{}", k.replace('~', "~0").replace('/', "~1"))).into();
    }
    match path {
        "metadata.name" => str_at(pod, "/metadata/name").into(),
        "metadata.namespace" => str_at(pod, "/metadata/namespace").into(),
        "metadata.uid" => str_at(pod, "/metadata/uid").into(),
        _ => String::new(),
    }
}

type Resolver<'a> = Option<&'a dyn Fn(&Value) -> String>;

fn containers(w: &mut W, label: &str, specs: &[Value], statuses: Option<&[Value]>, resolve: Resolver, space: &str) {
    let none = if specs.is_empty() { " <none>" } else { "" };
    out!(w, 0, "{space}{label}:{none}\n");
    let indent = if space.is_empty() { "" } else { " " };
    for c in specs {
        let name = str_at(c, "/name");
        let status = statuses.into_iter().flatten().find(|s| str_at(s, "/name") == name);
        out!(w, 1, "{indent}{name}:\n");
        if let Some(s) = status {
            out!(w, 2, "Container ID:\t{}\n", str_at(s, "/containerID"));
        }
        out!(w, 2, "Image:\t{}\n", str_at(c, "/image"));
        if let Some(s) = status {
            out!(w, 2, "Image ID:\t{}\n", str_at(s, "/imageID"));
        }
        let ports = container_ports(c, "/containerPort");
        if ports.contains(',') {
            out!(w, 2, "Ports:\t{ports}\n");
        } else {
            out!(w, 2, "Port:\t{}\n", or_none(&ports));
        }
        let host = container_ports(c, "/hostPort");
        if host.contains(',') {
            out!(w, 2, "Host Ports:\t{host}\n");
        } else {
            out!(w, 2, "Host Port:\t{}\n", or_none(&host));
        }
        let seccomp = str_at(c, "/securityContext/seccompProfile/type");
        if !seccomp.is_empty() {
            out!(w, 2, "SeccompProfile:\t{seccomp}\n");
            if seccomp == "Localhost" {
                out!(w, 3, "LocalhostProfile:\t{}\n", str_at(c, "/securityContext/seccompProfile/localhostProfile"));
            }
        }
        for (title, ptr) in [("Command", "/command"), ("Args", "/args")] {
            let list = arr(c, ptr);
            if !list.is_empty() {
                out!(w, 2, "{title}:\n");
                for part in list {
                    for s in part.as_str().unwrap_or_default().split('\n') {
                        out!(w, 3, "{s}\n");
                    }
                }
            }
        }
        if let Some(s) = status {
            container_state(w, "State", s.get("state").unwrap_or(&Value::Null));
            if s.pointer("/lastState/terminated").is_some() {
                container_state(w, "Last State", &s["lastState"]);
            }
            out!(w, 2, "Ready:\t{}\n", if flag(s, "/ready") { "True" } else { "False" });
            out!(w, 2, "Restart Count:\t{}\n", int(s, "/restartCount"));
        }
        if let Some(r) = c.get("resources") {
            resources(w, r, 2);
        }
        for (title, ptr) in [("Liveness", "/livenessProbe"), ("Readiness", "/readinessProbe"), ("Startup", "/startupProbe")] {
            if let Some(p) = c.pointer(ptr) {
                out!(w, 2, "{title}:\t{}\n", probe(p));
            }
        }
        env_from(w, c);
        env(w, c, resolve);
        mounts(w, c);
    }
}

fn container_ports(c: &Value, field: &str) -> String {
    arr(c, "/ports")
        .iter()
        .map(|p| {
            let s = format!("{}/{}", int(p, field), str_at(p, "/protocol"));
            match str_at(p, "/name") {
                "" => s,
                n => format!("{s} ({n})"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn container_state(w: &mut W, name: &str, state: &Value) {
    if let Some(r) = state.get("running") {
        out!(w, 2, "{name}:\tRunning\n");
        out!(w, 3, "Started:\t{}\n", rfc1123z(str_at(r, "/startedAt")));
    } else if let Some(s) = state.get("waiting") {
        out!(w, 2, "{name}:\tWaiting\n");
        let reason = str_at(s, "/reason");
        if !reason.is_empty() {
            out!(w, 3, "Reason:\t{reason}\n");
        }
    } else if let Some(t) = state.get("terminated") {
        out!(w, 2, "{name}:\tTerminated\n");
        for (label, ptr) in [("Reason", "/reason"), ("Message", "/message")] {
            let s = str_at(t, ptr);
            if !s.is_empty() {
                out!(w, 3, "{label}:\t{s}\n");
            }
        }
        out!(w, 3, "Exit Code:\t{}\n", int(t, "/exitCode"));
        if int(t, "/signal") > 0 {
            out!(w, 3, "Signal:\t{}\n", int(t, "/signal"));
        }
        out!(w, 3, "Started:\t{}\n", rfc1123z(str_at(t, "/startedAt")));
        out!(w, 3, "Finished:\t{}\n", rfc1123z(str_at(t, "/finishedAt")));
    } else {
        out!(w, 2, "{name}:\tWaiting\n");
    }
}

fn resources(w: &mut W, r: &Value, level: usize) {
    for (title, ptr) in [("Limits", "/limits"), ("Requests", "/requests")] {
        let kv = string_map(r, ptr);
        if !kv.is_empty() {
            out!(w, level, "{title}:\n");
        }
        for (k, v) in kv {
            out!(w, level + 1, "{k}:\t{v}\n");
        }
    }
}

/// An int-or-string, as Go's intstr prints it.
fn intstr(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => "0".into(),
    }
}

fn probe(p: &Value) -> String {
    let attrs = format!(
        "delay={}s timeout={}s period={}s successThreshold={} failureThreshold={}",
        int(p, "/initialDelaySeconds"),
        int(p, "/timeoutSeconds"),
        int(p, "/periodSeconds"),
        int(p, "/successThreshold"),
        int(p, "/failureThreshold")
    );
    if let Some(e) = p.get("exec") {
        let cmd: Vec<String> = arr(e, "/command").iter().map(go_v).collect();
        format!("exec [{}] {attrs}", cmd.join(" "))
    } else if let Some(h) = p.get("httpGet") {
        let port = h.get("port").map(intstr).unwrap_or_default();
        let host = str_at(h, "/host");
        let hostport = if port.is_empty() {
            host.to_string()
        } else if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        format!("http-get {}://{hostport}{} {attrs}", str_at(h, "/scheme").to_lowercase(), str_at(h, "/path"))
    } else if let Some(t) = p.get("tcpSocket") {
        format!("tcp-socket {}:{} {attrs}", str_at(t, "/host"), t.get("port").map(intstr).unwrap_or_default())
    } else if let Some(g) = p.get("grpc") {
        format!("grpc <pod>:{} {} {attrs}", int(g, "/port"), str_at(g, "/service"))
    } else {
        format!("unknown {attrs}")
    }
}

fn env_from(w: &mut W, c: &Value) {
    let list = arr(c, "/envFrom");
    if list.is_empty() {
        return;
    }
    w.s(2, "Environment Variables from:\n");
    for e in list {
        let (from, r) = if let Some(r) = e.get("configMapRef") {
            ("ConfigMap", r)
        } else if let Some(r) = e.get("secretRef") {
            ("Secret", r)
        } else {
            ("", &Value::Null)
        };
        let (name, optional) = (str_at(r, "/name"), flag(r, "/optional"));
        match str_at(e, "/prefix") {
            "" => out!(w, 3, "{name}\t{from}\tOptional: {optional}\n"),
            p => out!(w, 3, "{name}\t{from} with prefix '{p}'\tOptional: {optional}\n"),
        }
    }
}

fn env(w: &mut W, c: &Value, resolve: Resolver) {
    let list = arr(c, "/env");
    out!(w, 2, "Environment:{}\n", if list.is_empty() { "\t<none>" } else { "" });
    for e in list {
        let name = str_at(e, "/name");
        let Some(from) = e.get("valueFrom") else {
            for (i, s) in str_at(e, "/value").split('\n').enumerate() {
                if i == 0 {
                    out!(w, 3, "{name}:\t{s}\n");
                } else {
                    out!(w, 3, "\t{s}\n");
                }
            }
            continue;
        };
        if let Some(f) = from.get("fieldRef") {
            let value = resolve.map(|r| r(e)).unwrap_or_default();
            out!(w, 3, "{name}:\t{value} ({}:{})\n", str_at(f, "/apiVersion"), str_at(f, "/fieldPath"));
        } else if let Some(r) = from.get("resourceFieldRef") {
            let resource = str_at(r, "/resource");
            let mut value = resource_value(c, r);
            if value == "0" && (resource == "limits.cpu" || resource == "limits.memory") {
                value = "node allocatable".into();
            }
            out!(w, 3, "{name}:\t{value} ({resource})\n");
        } else if let Some(s) = from.get("secretKeyRef") {
            out!(w, 3, "{name}:\t<set to the key '{}' in secret '{}'>\tOptional: {}\n",
                    str_at(s, "/key"),
                    str_at(s, "/name"),
                    flag(s, "/optional"));
        } else if let Some(m) = from.get("configMapKeyRef") {
            out!(w, 3, "{name}:\t<set to the key '{}' of config map '{}'>\tOptional: {}\n",
                    str_at(m, "/key"),
                    str_at(m, "/name"),
                    flag(m, "/optional"));
        }
    }
}

/// A quantity such as `250m`, `64Mi` or `2` as a number.
fn quantity(s: &str) -> Option<f64> {
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, suffix) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let mult = match suffix {
        "" => 1.0,
        "m" => 1e-3,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "Ki" => 1024.0,
        "Mi" => 1024.0 * 1024.0,
        "Gi" => 1024.0 * 1024.0 * 1024.0,
        "Ti" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some(n * mult)
}

/// A container's resource as an env var sees it: its request or limit over
/// the divisor, rounded up, as kubectl's resource helper works it out.
fn resource_value(c: &Value, r: &Value) -> String {
    let resource = str_at(r, "/resource");
    let Some((section, name)) = resource.split_once('.') else {
        return String::new();
    };
    let value = c
        .pointer(&format!("/resources/{section}/{name}"))
        .and_then(Value::as_str)
        .and_then(quantity)
        .unwrap_or(0.0);
    let divisor = match str_at(r, "/divisor") {
        "" | "0" => 1.0,
        d => quantity(d).unwrap_or(1.0),
    };
    format!("{}", (value / divisor).ceil() as i64)
}

fn mounts(w: &mut W, c: &Value) {
    let list = sorted_by(arr(c, "/volumeMounts"), "/mountPath");
    let none = if list.is_empty() { "\t<none>" } else { "" };
    out!(w, 2, "Mounts:{none}\n");
    for m in list {
        let mut flags = vec![if flag(m, "/readOnly") { "ro".to_string() } else { "rw".to_string() }];
        let sub = str_at(m, "/subPath");
        if !sub.is_empty() {
            flags.push(format!("path={}", Value::from(sub)));
        }
        out!(w, 3, "{} from {} ({})\n", str_at(m, "/mountPath"), str_at(m, "/name"), flags.join(","));
    }
    let devices = sorted_by(arr(c, "/volumeDevices"), "/devicePath");
    if !devices.is_empty() {
        out!(w, 2, "Devices:{none}\n");
        for d in devices {
            out!(w, 3, "{} from {}\n", str_at(d, "/devicePath"), str_at(d, "/name"));
        }
    }
}

fn volumes(w: &mut W, list: &[Value], space: &str) {
    if list.is_empty() {
        out!(w, 0, "{space}Volumes:\t<none>\n");
        return;
    }
    out!(w, 0, "{space}Volumes:\n");
    let indent = if space.is_empty() { "" } else { " " };
    for v in list {
        out!(w, 1, "{indent}{}:\n", str_at(v, "/name"));
        let opt = |s: &Value| flag(s, "/optional");
        if let Some(h) = v.get("hostPath") {
            let t = match str_at(h, "/type") {
                "" => "<none>",
                t => t,
            };
            out!(w, 2, "Type:\tHostPath (bare host directory volume)\n    Path:\t{}\n    HostPathType:\t{t}\n", str_at(h, "/path"));
        } else if let Some(e) = v.get("emptyDir") {
            let limit = match str_at(e, "/sizeLimit") {
                "" | "0" => "<unset>",
                l => l,
            };
            out!(w, 2, "Type:\tEmptyDir (a temporary directory that shares a pod's lifetime)\n    Medium:\t{}\n    SizeLimit:\t{limit}\n", str_at(e, "/medium"));
        } else if let Some(g) = v.get("gitRepo") {
            out!(w, 2, "Type:\tGitRepo (a volume that is pulled from git when the pod is created)\n    Repository:\t{}\n    Revision:\t{}\n",
                    str_at(g, "/repository"),
                    str_at(g, "/revision"));
        } else if let Some(s) = v.get("secret") {
            out!(w, 2, "Type:\tSecret (a volume populated by a Secret)\n    SecretName:\t{}\n    Optional:\t{}\n", str_at(s, "/secretName"), opt(s));
        } else if let Some(c) = v.get("configMap") {
            out!(w, 2, "Type:\tConfigMap (a volume populated by a ConfigMap)\n    Name:\t{}\n    Optional:\t{}\n", str_at(c, "/name"), opt(c));
        } else if let Some(n) = v.get("nfs") {
            out!(w, 2, "Type:\tNFS (an NFS mount that lasts the lifetime of a pod)\n    Server:\t{}\n    Path:\t{}\n    ReadOnly:\t{}\n",
                    str_at(n, "/server"),
                    str_at(n, "/path"),
                    flag(n, "/readOnly"));
        } else if let Some(c) = v.get("persistentVolumeClaim") {
            out!(w, 2, "Type:\tPersistentVolumeClaim (a reference to a PersistentVolumeClaim in the same namespace)\n    ClaimName:\t{}\n    ReadOnly:\t{}\n",
                    str_at(c, "/claimName"),
                    flag(c, "/readOnly"));
        } else if v.get("ephemeral").is_some() {
            w.s(2, "Type:\tEphemeralVolume (an inline specification for a volume that gets created and deleted with the pod)\n");
        } else if let Some(d) = v.get("downwardAPI") {
            w.s(2, "Type:\tDownwardAPI (a volume populated by information about the pod)\n    Items:\n");
            for i in arr(d, "/items") {
                if let Some(f) = i.get("fieldRef") {
                    out!(w, 3, "{} -> {}\n", str_at(f, "/fieldPath"), str_at(i, "/path"));
                }
                if let Some(r) = i.get("resourceFieldRef") {
                    out!(w, 3, "{} -> {}\n", str_at(r, "/resource"), str_at(i, "/path"));
                }
            }
        } else if let Some(p) = v.get("projected") {
            w.s(2, "Type:\tProjected (a volume that contains injected data from multiple sources)\n");
            for s in arr(p, "/sources") {
                if let Some(x) = s.get("secret") {
                    out!(w, 2, "SecretName:\t{}\n    Optional:\t{}\n", str_at(x, "/name"), opt(x));
                } else if s.get("downwardAPI").is_some() {
                    w.s(2, "DownwardAPI:\ttrue\n");
                } else if let Some(x) = s.get("configMap") {
                    out!(w, 2, "ConfigMapName:\t{}\n    Optional:\t{}\n", str_at(x, "/name"), opt(x));
                } else if let Some(x) = s.get("serviceAccountToken") {
                    out!(w, 2, "TokenExpirationSeconds:\t{}\n", int(x, "/expirationSeconds"));
                }
            }
        } else if let Some(c) = v.get("csi") {
            out!(w, 2, "Type:\tCSI (a Container Storage Interface (CSI) volume source)\n    Driver:\t{}\n    FSType:\t{}\n    ReadOnly:\t{}\n",
                    str_at(c, "/driver"),
                    str_at(c, "/fsType"),
                    flag(c, "/readOnly"));
            w.s(2, "VolumeAttributes:\t");
            let kv = string_map(c, "/volumeAttributes");
            if kv.is_empty() {
                w.s(0, "<none>\n");
            }
            for (i, (k, v)) in kv.iter().enumerate() {
                if i != 0 {
                    w.s(2, "\t");
                }
                out!(w, 2, "{}\n", shorten(&format!("{k}={v}"), MAX_ANNOTATION_LEN));
            }
        } else if let Some(i) = v.get("image") {
            out!(w, 2, "Type:\tImage (a container image or OCI artifact)\n    Reference:\t{}\n    PullPolicy:\t{}\n",
                    str_at(i, "/reference"),
                    str_at(i, "/pullPolicy"));
        } else {
            // Cloud and legacy volume types this tool doesn't spell out.
            w.s(1, "<unknown>\n");
        }
    }
}

fn topology_spread(w: &mut W, list: &[Value], space: &str) {
    if list.is_empty() {
        return;
    }
    let list = sorted_by(list, "/topologyKey");
    out!(w, 0, "{space}Topology Spread Constraints:\t");
    for (i, t) in list.iter().enumerate() {
        if i != 0 {
            w.s(0, space);
            w.s(0, "\t");
        }
        let mut s = format!(
            "{}:{} when max skew {} is exceeded",
            str_at(t, "/topologyKey"),
            str_at(t, "/whenUnsatisfiable"),
            int(t, "/maxSkew")
        );
        if let Some(sel) = t.get("labelSelector").filter(|s| !s.is_null()) {
            s.push_str(&format!(" for selector {}", or_none(&selector(sel))));
        }
        out!(w, 0, "{s}\n");
    }
}

// ---------------------------------------------------------------------------
// Deployments, Services, Secrets, ConfigMaps, Namespaces.
// ---------------------------------------------------------------------------

fn deployment(w: &mut W, o: &Value, x: &Extra, now: i64) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    out!(w, 0, "Namespace:\t{}\n", str_at(o, "/metadata/namespace"));
    out!(w, 0, "CreationTimestamp:\t{}\n", rfc1123z(str_at(o, "/metadata/creationTimestamp")));
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    out!(w, 0, "Selector:\t{}\n", selector(o.pointer("/spec/selector").unwrap_or(&Value::Null)));
    out!(w, 0, "Replicas:\t{} desired | {} updated | {} total | {} available | {} unavailable\n",
            int(o, "/spec/replicas"),
            int(o, "/status/updatedReplicas"),
            int(o, "/status/replicas"),
            int(o, "/status/availableReplicas"),
            int(o, "/status/unavailableReplicas"));
    out!(w, 0, "StrategyType:\t{}\n", str_at(o, "/spec/strategy/type"));
    out!(w, 0, "MinReadySeconds:\t{}\n", int(o, "/spec/minReadySeconds"));
    if let Some(ru) = o.pointer("/spec/strategy/rollingUpdate") {
        let f = |k: &str| ru.get(k).map(intstr).unwrap_or_else(|| "0".into());
        out!(w, 0, "RollingUpdateStrategy:\t{} max unavailable, {} max surge\n", f("maxUnavailable"), f("maxSurge"));
    }
    pod_template(w, o.pointer("/spec/template").unwrap_or(&Value::Null));
    let conditions = arr(o, "/status/conditions");
    if !conditions.is_empty() {
        w.s(0, "Conditions:\n  Type\tStatus\tReason\n");
        w.s(1, "----\t------\t------\n");
        for c in conditions {
            out!(w, 1, "{} \t{}\t{}\n", str_at(c, "/type"), str_at(c, "/status"), str_at(c, "/reason"));
        }
    }
    if let Some((old, new)) = &x.replica_sets {
        if !old.is_empty() || !new.is_empty() {
            let list = |rs: &[Value]| {
                let s: Vec<String> = rs
                    .iter()
                    .map(|r| format!("{} ({}/{} replicas created)", str_at(r, "/metadata/name"), int(r, "/status/replicas"), int(r, "/spec/replicas")))
                    .collect();
                or_none(&s.join(", "))
            };
            out!(w, 0, "OldReplicaSets:\t{}\n", list(old));
            out!(w, 0, "NewReplicaSet:\t{}\n", list(new));
        }
    }
    events(w, x, now);
}

fn pod_template(w: &mut W, t: &Value) {
    w.s(0, "Pod Template:\n");
    if t.is_null() {
        w.s(1, "<unset>");
        return;
    }
    labels_multiline(w, "  Labels", t, "/metadata/labels");
    if obj(t, "/metadata/annotations").is_some() {
        annotations_multiline(w, "  Annotations", t, "/metadata/annotations");
    }
    let sa = str_at(t, "/spec/serviceAccountName");
    if !sa.is_empty() {
        out!(w, 1, "Service Account:\t{sa}\n");
    }
    if !arr(t, "/spec/initContainers").is_empty() {
        containers(w, "Init Containers", arr(t, "/spec/initContainers"), None, None, "  ");
    }
    containers(w, "Containers", arr(t, "/spec/containers"), None, None, "  ");
    volumes(w, arr(t, "/spec/volumes"), "  ");
    topology_spread(w, arr(t, "/spec/topologySpreadConstraints"), "  ");
    let pc = str_at(t, "/spec/priorityClassName");
    if !pc.is_empty() {
        out!(w, 1, "Priority Class Name:\t{pc}\n");
    }
    labels_multiline(w, "  Node-Selectors", t, "/spec/nodeSelector");
    tolerations_multiline(w, "  Tolerations", t, "/spec/tolerations");
    if let Some(g) = t.pointer("/spec/schedulingGroup").filter(|g| !g.is_null()) {
        w.s(0, "  SchedulingGroup:\n");
        out!(w, 1, "PodGroupName:\t{}\n", str_at(g, "/podGroupName"));
    }
}

fn service(w: &mut W, o: &Value, x: &Extra, now: i64) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    out!(w, 0, "Namespace:\t{}\n", str_at(o, "/metadata/namespace"));
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    out!(w, 0, "Selector:\t{}\n", format_labels(o, "/spec/selector"));
    out!(w, 0, "Type:\t{}\n", str_at(o, "/spec/type"));
    let policy = str_at(o, "/spec/ipFamilyPolicy");
    if !policy.is_empty() {
        out!(w, 0, "IP Family Policy:\t{policy}\n");
    }
    let join = |ptr: &str| arr(o, ptr).iter().filter_map(Value::as_str).collect::<Vec<_>>().join(",");
    out!(w, 0, "IP Families:\t{}\n", or_none(&join("/spec/ipFamilies")));
    out!(w, 0, "IP:\t{}\n", str_at(o, "/spec/clusterIP"));
    out!(w, 0, "IPs:\t{}\n", or_none(&join("/spec/clusterIPs")));
    let external = join("/spec/externalIPs");
    if !external.is_empty() {
        out!(w, 0, "External IPs:\t{external}\n");
    }
    let lb_ip = str_at(o, "/spec/loadBalancerIP");
    if !lb_ip.is_empty() {
        out!(w, 0, "Desired LoadBalancer IP:\t{lb_ip}\n");
    }
    let ext_name = str_at(o, "/spec/externalName");
    if !ext_name.is_empty() {
        out!(w, 0, "External Name:\t{ext_name}\n");
    }
    let ingress = arr(o, "/status/loadBalancer/ingress");
    if !ingress.is_empty() {
        let list: Vec<String> = ingress
            .iter()
            .map(|i| match str_at(i, "/ip") {
                "" => str_at(i, "/hostname").to_string(),
                ip => match str_at(i, "/ipMode") {
                    "" => ip.to_string(),
                    m => format!("{ip} ({m})"),
                },
            })
            .collect();
        out!(w, 0, "LoadBalancer Ingress:\t{}\n", list.join(", "));
    }
    for p in arr(o, "/spec/ports") {
        let name = match str_at(p, "/name") {
            "" => "<unset>",
            n => n,
        };
        let proto = str_at(p, "/protocol");
        out!(w, 0, "Port:\t{name}\t{}/{proto}\n", int(p, "/port"));
        out!(w, 0, "TargetPort:\t{}/{proto}\n", p.get("targetPort").map(intstr).unwrap_or_else(|| "0".into()));
        let app = str_at(p, "/appProtocol");
        if !app.is_empty() {
            out!(w, 0, "AppProtocol:\t{app}\n");
        }
        if int(p, "/nodePort") != 0 {
            out!(w, 0, "NodePort:\t{name}\t{}/{proto}\n", int(p, "/nodePort"));
        }
        out!(w, 0, "Endpoints:\t{}\n", endpoints(&x.endpoint_slices, str_at(p, "/name")));
    }
    out!(w, 0, "Session Affinity:\t{}\n", str_at(o, "/spec/sessionAffinity"));
    for (label, ptr) in [
        ("External Traffic Policy", "/spec/externalTrafficPolicy"),
        ("Internal Traffic Policy", "/spec/internalTrafficPolicy"),
    ] {
        let s = str_at(o, ptr);
        if !s.is_empty() {
            out!(w, 0, "{label}:\t{s}\n");
        }
    }
    if int(o, "/spec/healthCheckNodePort") != 0 {
        out!(w, 0, "HealthCheck NodePort:\t{}\n", int(o, "/spec/healthCheckNodePort"));
    }
    let ranges = join("/spec/loadBalancerSourceRanges");
    if !ranges.is_empty() {
        out!(w, 0, "LoadBalancer Source Ranges:\t{ranges}\n");
    }
    let td = str_at(o, "/spec/trafficDistribution");
    if !td.is_empty() {
        out!(w, 0, "Traffic Distribution:\t{td}\n");
    }
    events(w, x, now);
}

/// formatEndpointSlices for one port name: up to three ready addresses,
/// then "+ N more...".
fn endpoints(slices: &[Value], port_name: &str) -> String {
    if slices.is_empty() {
        return "<none>".into();
    }
    let (mut list, mut more, mut count) = (Vec::new(), false, 0);
    let ready = |e: &Value| e.pointer("/conditions/ready").and_then(Value::as_bool).unwrap_or(true);
    for s in slices {
        let ports = arr(s, "/ports");
        if ports.is_empty() {
            for e in arr(s, "/endpoints") {
                if list.len() == 3 {
                    more = true;
                }
                if !ready(e) {
                    continue;
                }
                if !more {
                    list.push(str_at(e, "/addresses/0").to_string());
                }
                count += 1;
            }
            continue;
        }
        for p in ports.iter().filter(|p| str_at(p, "/name") == port_name) {
            for e in arr(s, "/endpoints") {
                if list.len() == 3 {
                    more = true;
                }
                if !ready(e) {
                    continue;
                }
                if !more {
                    let addr = str_at(e, "/addresses/0");
                    let addr = if addr.contains(':') { format!("[{addr}]") } else { addr.to_string() };
                    list.push(format!("{addr}:{}", int(p, "/port")));
                }
                count += 1;
            }
        }
    }
    let joined = list.join(",");
    if more { format!("{joined} + {} more...", count - 3) } else { joined }
}

/// Keys and sizes. As kubectl does, a service account token's `token` is
/// printed in full: whoever may describe the Secret may read it anyway.
fn secret(w: &mut W, o: &Value) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    out!(w, 0, "Namespace:\t{}\n", str_at(o, "/metadata/namespace"));
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    let ty = str_at(o, "/type");
    out!(w, 0, "\nType:\t{ty}\n");
    w.s(0, "\nData\n====\n");
    for (k, v) in string_map(o, "/data") {
        if k == "token" && ty == "kubernetes.io/service-account-token" {
            out!(w, 0, "{k}:\t{}\n", String::from_utf8_lossy(&b64_decode(&v)));
        } else {
            out!(w, 0, "{k}:\t{} bytes\n", b64_len(&v));
        }
    }
}

/// As kubectl's: a ConfigMap's values are printed, a binary value's size.
fn config_map(w: &mut W, o: &Value, x: &Extra, now: i64) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    out!(w, 0, "Namespace:\t{}\n", str_at(o, "/metadata/namespace"));
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    w.s(0, "\nData\n====\n");
    for (k, v) in string_map(o, "/data") {
        out!(w, 0, "{k}:\n----\n");
        out!(w, 0, "{v}\n");
        w.s(0, "\n");
    }
    w.s(0, "\nBinaryData\n====\n");
    for (k, v) in string_map(o, "/binaryData") {
        out!(w, 0, "{k}: {} bytes\n", b64_len(&v));
    }
    w.s(0, "\n");
    events(w, x, now);
}

fn namespace(w: &mut W, o: &Value, x: &Extra) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    out!(w, 0, "Status:\t{}\n", str_at(o, "/status/phase"));
    let conditions = arr(o, "/status/conditions");
    if !conditions.is_empty() {
        w.s(0, "Conditions:\n");
        w.s(1, "Type\tStatus\tLastTransitionTime\tReason\tMessage\n");
        w.s(1, "----\t------\t------------------\t------\t-------\n");
        for c in conditions {
            out!(w, 1, "{}\t{}\t{}\t{}\t{}\n",
                    str_at(c, "/type"),
                    str_at(c, "/status"),
                    rfc1123z(str_at(c, "/lastTransitionTime")),
                    str_at(c, "/reason"),
                    str_at(c, "/message"));
        }
    }
    if let Some(quotas) = &x.quotas {
        w.s(0, "\n");
        if quotas.is_empty() {
            w.s(0, "No resource quota.\n");
        } else {
            let quotas = sorted_by(quotas, "/metadata/name");
            w.s(0, "Resource Quotas\n");
            for q in quotas {
                out!(w, 1, "Name:\t{}\n", str_at(q, "/metadata/name"));
                let mut scopes: Vec<&str> = arr(q, "/spec/scopes").iter().filter_map(Value::as_str).collect();
                if !scopes.is_empty() {
                    sort_strs(&mut scopes);
                    out!(w, 1, "Scopes:\t{}\n", scopes.join(", "));
                    for s in scopes {
                        let help = match s {
                            "Terminating" => "Matches all pods that have an active deadline. These pods have a limited lifespan on a node before being actively terminated by the system.",
                            "NotTerminating" => "Matches all pods that do not have an active deadline. These pods usually include long running pods whose container command is not expected to terminate.",
                            "BestEffort" => "Matches all pods that do not have resource requirements set. These pods have a best effort quality of service.",
                            "NotBestEffort" => "Matches all pods that have at least one resource requirement set. These pods have a burstable or guaranteed quality of service.",
                            _ => "",
                        };
                        if !help.is_empty() {
                            out!(w, 1, "* {help}\n");
                        }
                    }
                }
                w.s(1, "Resource\tUsed\tHard\n");
                w.s(1, "--------\t---\t---\n");
                let used = string_map(q, "/status/used");
                for (r, hard) in string_map(q, "/status/hard") {
                    let u = used.iter().find(|(k, _)| *k == r).map_or("0", |(_, v)| v.as_str());
                    out!(w, 1, "{r}\t{u}\t{hard}\n");
                }
            }
        }
    }
    if let Some(ranges) = &x.limit_ranges {
        w.s(0, "\n");
        if ranges.is_empty() {
            w.s(0, "No LimitRange resource.\n");
        } else {
            w.s(0, "Resource Limits\n Type\tResource\tMin\tMax\tDefault Request\tDefault Limit\tMax Limit/Request Ratio\n");
            w.s(0, " ----\t--------\t---\t---\t---------------\t-------------\t-----------------------\n");
            for lr in ranges {
                for item in arr(lr, "/spec/limits") {
                    let fields = ["/min", "/max", "/defaultRequest", "/default", "/maxLimitRequestRatio"];
                    let all: Vec<(String, String)> = fields.iter().flat_map(|f| string_map(item, f)).collect();
                    let mut names: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
                    sort_strs(&mut names);
                    names.dedup();
                    for n in names {
                        let get = |f: &str| string_map(item, f).into_iter().find(|(k, _)| *k == n).map_or("-".to_string(), |(_, v)| v);
                        out!(w, 0, " {}\t{n}\t{}\t{}\t{}\t{}\t{}\n",
                                str_at(item, "/type"),
                                get("/min"),
                                get("/max"),
                                get("/defaultRequest"),
                                get("/default"),
                                get("/maxLimitRequestRatio"));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Any other kind, such as a WorkloadDeployment: the generic describer.
// ---------------------------------------------------------------------------

fn generic(w: &mut W, o: &Value, x: &Extra, now: i64) {
    out!(w, 0, "Name:\t{}\n", str_at(o, "/metadata/name"));
    out!(w, 0, "Namespace:\t{}\n", str_at(o, "/metadata/namespace"));
    labels_multiline(w, "Labels", o, "/metadata/labels");
    annotations_multiline(w, "Annotations", o, "/metadata/annotations");
    if let Some(m) = o.as_object() {
        let skip = [".metadata.managedFields", ".metadata.name", ".metadata.namespace", ".metadata.labels", ".metadata.annotations"];
        unstructured(w, 0, m, "", &skip);
    }
    events(w, x, now);
}

fn unstructured(w: &mut W, level: usize, content: &Map<String, Value>, prefix: &str, skip: &[&str]) {
    let mut fields: Vec<&str> = content.keys().map(String::as_str).collect();
    sort_strs(&mut fields);
    for field in fields {
        let path = format!("{prefix}.{field}");
        if skip.contains(&path.as_str()) {
            continue;
        }
        match &content[field] {
            Value::Object(m) => {
                out!(w, level, "{}:\n", smart_label(field));
                unstructured(w, level + 1, m, &path, skip);
            }
            Value::Array(a) => {
                out!(w, level, "{}:\n", smart_label(field));
                for child in a {
                    match child {
                        Value::Object(m) => unstructured(w, level + 1, m, &path, skip),
                        v => out!(w, level + 1, "{}\n", go_v(v)),
                    }
                }
            }
            v => out!(w, level, "{}:\t{}\n", smart_label(field), go_v(v)),
        }
    }
}

/// kubectl's smartLabelFor: `creationTimestamp` is `Creation Timestamp`,
/// `apiVersion` is `API Version`. A field with anything but (ASCII) letters
/// and `-` stays as it is.
fn smart_label(field: &str) -> String {
    if field.chars().any(|c| !c.is_ascii_alphabetic() && c != '-') {
        return field.into();
    }
    let parts = camel_split(field);
    let mut merged: Vec<String> = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let part = &parts[i];
        if let Some(next) = parts.get(i + 1) {
            let upper = part.bytes().all(|c| c.is_ascii_uppercase());
            if part.len() >= 2 && upper && next.bytes().next().is_some_and(|c| c.is_ascii_uppercase()) {
                merged.push(format!("{part}{next}"));
                i += 2;
                continue;
            }
        }
        merged.push(part.clone());
        i += 1;
    }
    merged
        .into_iter()
        .filter(|p| p != "_")
        .map(|p| {
            if ["API", "URL", "UID", "GUID"].contains(&p.to_ascii_uppercase().as_str()) {
                p.to_ascii_uppercase()
            } else if p.bytes().all(|c| c.is_ascii_lowercase()) {
                p[..1].to_ascii_uppercase() + &p[1..]
            } else {
                p
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// fatih/camelcase's Split: runs of lower, upper, digit and other characters,
/// with an upper-to-lower boundary moved back one (`PDFLoader`: PDF Loader).
fn camel_split(s: &str) -> Vec<String> {
    let class = |c: char| match () {
        _ if c.is_ascii_lowercase() => 1,
        _ if c.is_ascii_uppercase() => 2,
        _ if c.is_ascii_digit() => 3,
        _ => 4,
    };
    let mut runs: Vec<Vec<char>> = Vec::new();
    let mut last = 0;
    for c in s.chars() {
        let k = class(c);
        match runs.last_mut() {
            Some(r) if k == last => r.push(c),
            _ => runs.push(vec![c]),
        }
        last = k;
    }
    for i in 0..runs.len().saturating_sub(1) {
        if runs[i].first().is_some_and(char::is_ascii_uppercase) && runs[i + 1].first().is_some_and(char::is_ascii_lowercase) {
            if let Some(c) = runs[i].pop() {
                runs[i + 1].insert(0, c);
            }
        }
    }
    runs.into_iter().filter(|r| !r.is_empty()).map(|r| r.into_iter().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabwriter_like_go() {
        assert_eq!(tabwriter("a:\tx\nbbb:\ty\nc:\n  d:\tz\n"), "a:    x\nbbb:  y\nc:\n  d:  z\n");
        assert_eq!(tabwriter("Medium:\t\nSize:\t<unset>\n"), "Medium:  \nSize:    <unset>\n");
    }

    #[test]
    fn times_as_rfc1123z() {
        assert_eq!(rfc1123z("2026-11-05T01:40:29Z"), "Thu, 05 Nov 2026 01:40:29 +0000");
        assert_eq!(rfc1123z("2024-02-29T23:59:59Z"), "Thu, 29 Feb 2024 23:59:59 +0000");
        assert_eq!(rfc1123z(""), "Mon, 01 Jan 0001 00:00:00 +0000");
    }

    #[test]
    fn smart_labels() {
        for (f, want) in [
            ("creationTimestamp", "Creation Timestamp"),
            ("apiVersion", "API Version"),
            ("uid", "UID"),
            ("hostgroup", "Hostgroup"),
            ("readOnly", "Read Only"),
            ("podIPs", "Pod I Ps"),
            ("k8s", "k8s"),
            ("app.kubernetes.io/name", "app.kubernetes.io/name"),
        ] {
            assert_eq!(smart_label(f), want, "{f}");
        }
    }

    /// Objects from a kind cluster, against `kubectl describe` (v1.37.1,
    /// TZ=UTC, --show-events=false) of the same objects.
    #[test]
    fn matches_kubectl() {
        // The pod and the deployment as agent-reader describes them: events
        // and replica sets are refused, so the events list is empty and the
        // replica sets are left out.
        let first_pod = || {
            let pods: Value = serde_json::from_str(include_str!("testdata/pods.json")).unwrap();
            pods[0].to_string()
        };
        let refused = || Extra { events: Some(Vec::new()), ..Extra::default() };
        let slices = || Extra {
            endpoint_slices: serde_json::from_str(include_str!("testdata/service-endpointslices.json")).unwrap(),
            ..Extra::default()
        };
        let pod = first_pod();
        for (kind, json, want, x) in [
            ("Pod", pod.as_str(), include_str!("testdata/describe-pod.txt"), refused()),
            ("Pod", include_str!("testdata/pod-hostgroup.json"), include_str!("testdata/pod-hostgroup.txt"), Extra::default()),
            ("Deployment", include_str!("testdata/deployment.json"), include_str!("testdata/describe-deployment.txt"), refused()),
            ("Service", include_str!("testdata/service.json"), include_str!("testdata/service.txt"), slices()),
            ("Secret", include_str!("testdata/secret.json"), include_str!("testdata/secret.txt"), Extra::default()),
            ("WorkloadDeployment", include_str!("testdata/workloaddeployment.json"), include_str!("testdata/workloaddeployment.txt"), Extra::default()),
        ] {
            let o: Value = serde_json::from_str(json).unwrap();
            let got = describe(kind, &o, &x, 0);
            assert_eq!(got, want, "{kind} {}", str_at(&o, "/metadata/name"));
        }
    }
}
