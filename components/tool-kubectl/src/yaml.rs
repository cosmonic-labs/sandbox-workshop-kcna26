//! `-o yaml`, written by hand: a YAML library would add far more to the
//! component than this does. It follows what kubectl prints (sigs.k8s.io/yaml
//! over go-yaml v2): two-space indents, list items level with their key, keys
//! in go-yaml's order, `|` blocks for multi-line strings, and quotes only where
//! a plain string would read as something else, and long strings folded at
//! spaces past column 80, as go-yaml does.

use serde_json::Value;

pub fn to_string(v: &Value) -> String {
    let mut out = String::new();
    match v {
        Value::Object(m) if !m.is_empty() => map(v, 0, &mut out),
        Value::Array(a) if !a.is_empty() => list(a, 0, &mut out),
        _ => {
            out.push_str(&scalar(v, 0, Some(0)));
            out.push('\n');
        }
    }
    out
}

/// The column the next character lands in.
fn column(out: &str) -> usize {
    out.len() - out.rfind('\n').map_or(0, |i| i + 1)
}

fn pad(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

/// A mapping's entries, each at `indent`.
fn map(v: &Value, indent: usize, out: &mut String) {
    let Value::Object(m) = v else { return };
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort_by(|a, b| go_order(a, b));
    for (i, k) in keys.into_iter().enumerate() {
        // The first key of a list item sits on the "- " line.
        if i > 0 || !out.ends_with("- ") {
            pad(out, indent);
        }
        out.push_str(&string(k, indent, None));
        out.push(':');
        value(&m[k], indent, out);
    }
}

/// A list's items at `indent`, each starting "- ".
fn list(a: &[Value], indent: usize, out: &mut String) {
    for item in a {
        if !out.ends_with("- ") {
            pad(out, indent);
        }
        out.push_str("- ");
        match item {
            Value::Object(m) if !m.is_empty() => map(item, indent + 2, out),
            Value::Array(l) if !l.is_empty() => list(l, indent + 2, out),
            _ => {
                out.push_str(&scalar(item, indent + 2, Some(column(out))));
                out.push('\n');
            }
        }
    }
}

/// What follows `key:`: a nested block on the next lines, or a scalar.
fn value(v: &Value, indent: usize, out: &mut String) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            out.push('\n');
            map(v, indent + 2, out);
        }
        // go-yaml writes a list level with its key.
        Value::Array(a) if !a.is_empty() => {
            out.push('\n');
            list(a, indent, out);
        }
        _ => {
            out.push(' ');
            out.push_str(&scalar(v, indent + 2, Some(column(out))));
            out.push('\n');
        }
    }
}

/// `col` is where a value starts, for folding; keys (`None`) never fold.
fn scalar(v: &Value, indent: usize, col: Option<usize>) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => string(s, indent, col),
        Value::Object(_) => "{}".into(),
        Value::Array(_) => "[]".into(),
    }
}

/// A string as go-yaml writes it: a `|` block if it has line breaks, plain if
/// that reads back as the same string, otherwise quoted.
fn string(s: &str, indent: usize, col: Option<usize>) -> String {
    if s.contains('\n') && !s.contains(" \n") && !s.starts_with([' ', '\n']) {
        let chomp = match s.len() - s.trim_end_matches('\n').len() {
            0 => "-",
            1 => "",
            _ => "+",
        };
        let mut out = format!("|{chomp}");
        for line in s.strip_suffix('\n').unwrap_or(s).split('\n') {
            out.push('\n');
            if !line.is_empty() {
                pad(&mut out, indent);
                out.push_str(line);
            }
        }
        return out;
    }
    let out = if s.chars().any(|c| c.is_control()) || reads_as_other_type(s) {
        double(s)
    } else if plain_ok(s) {
        s.into()
    } else {
        format!("'{}'", s.replace('\'', "''"))
    };
    match col {
        Some(col) => fold(&out, col, indent),
        None => out,
    }
}

/// go-yaml's line breaking (best width 80): a single space reached past
/// column 80 becomes a line break and the value's indent, unless it is the
/// first or last character or another space follows.
fn fold(s: &str, mut col: usize, indent: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut spaces = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && !spaces && col > 80 && i > 0 && i + 1 < chars.len() && chars[i + 1] != ' ' {
            out.push('\n');
            pad(&mut out, indent);
            col = indent;
            spaces = true;
            continue;
        }
        spaces = c == ' ';
        out.push(c);
        col += 1;
    }
    out
}

fn double(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Would this string, unquoted, read back as null, a bool, a number or a
/// timestamp? Then go-yaml quotes it, with double quotes.
fn reads_as_other_type(s: &str) -> bool {
    const WORDS: &[&str] = &[
        "", "~", "null", "Null", "NULL", "y", "Y", "yes", "Yes", "YES", "n", "N", "no", "No", "NO",
        "true", "True", "TRUE", "false", "False", "FALSE", "on", "On", "ON", "off", "Off", "OFF",
        ".inf", ".Inf", ".INF", "+.inf", "+.Inf", "+.INF", "-.inf", "-.Inf", "-.INF", ".nan", ".NaN", ".NAN",
    ];
    if WORDS.contains(&s) {
        return true;
    }
    let first = s.as_bytes()[0];
    if !(first.is_ascii_digit() || matches!(first, b'+' | b'-' | b'.')) {
        return false;
    }
    let digits = s.trim_start_matches(['+', '-']);
    let is_int = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        || digits.strip_prefix("0x").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
        || digits.strip_prefix("0o").is_some_and(|o| !o.is_empty() && o.bytes().all(|b| (b'0'..=b'7').contains(&b)))
        || digits.strip_prefix("0b").is_some_and(|o| !o.is_empty() && o.bytes().all(|b| b == b'0' || b == b'1'));
    let is_float = s.parse::<f64>().is_ok() && !s.eq_ignore_ascii_case("inf") && !s.eq_ignore_ascii_case("nan");
    // A date: 2026-11-05, or a date and time.
    let b = s.as_bytes();
    let is_date = b.len() >= 8 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' && {
        let rest = &s[5..];
        let mut parts = rest.splitn(3, ['-', 'T', 't', ' ']);
        let ok = |p: Option<&str>| p.is_some_and(|p| (1..=2).contains(&p.len()) && p.bytes().all(|c| c.is_ascii_digit()));
        ok(parts.next()) && ok(parts.next().map(|p| &p[..p.len().min(2)]))
    };
    // A sexagesimal number such as 1:20, which YAML 1.1 reads as 80
    // (go-yaml's isBase60Float: [-+]?[0-9][0-9_]*(:[0-5]?[0-9])+(\.[0-9_]*)?).
    let base60 = {
        let mut parts = s.trim_start_matches(['+', '-']).split(':');
        let head = parts.next().unwrap_or_default();
        let mut rest: Vec<&str> = parts.collect();
        let frac_ok = match rest.last_mut().and_then(|l| l.split_once('.')) {
            Some((sexa, frac)) => {
                let ok = frac.bytes().all(|c| c.is_ascii_digit() || c == b'_');
                *rest.last_mut().unwrap() = sexa;
                ok
            }
            None => true,
        };
        !rest.is_empty()
            && frac_ok
            && head.bytes().next().is_some_and(|c| c.is_ascii_digit())
            && head.bytes().all(|c| c.is_ascii_digit() || c == b'_')
            && rest.iter().all(|p| (1..=2).contains(&p.len()) && p.bytes().all(|c| c.is_ascii_digit()) && p.parse::<u8>().is_ok_and(|n| n < 60))
    };
    is_int || is_float || is_date || base60
}

/// go-yaml's test for a plain scalar in block context.
fn plain_ok(s: &str) -> bool {
    if s.starts_with("---") || s.starts_with("...") || s.starts_with(' ') || s.ends_with(' ') {
        return false;
    }
    let b = s.as_bytes();
    let blank_after = |i: usize| i + 1 >= b.len() || b[i + 1] == b' ';
    for (i, &c) in b.iter().enumerate() {
        let bad = if i == 0 {
            matches!(c, b'#' | b',' | b'[' | b']' | b'{' | b'}' | b'&' | b'*' | b'!' | b'|' | b'>' | b'\'' | b'"' | b'%' | b'@' | b'`')
                || (matches!(c, b'?' | b':' | b'-') && blank_after(i))
        } else {
            (c == b':' && blank_after(i)) || (c == b'#' && b[i - 1] == b' ')
        };
        if bad {
            return false;
        }
    }
    true
}

/// go-yaml's key order (its keyList.Less): non-letters before letters, and
/// digit runs compared by value.
fn go_order(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    for i in 0..a.len().min(b.len()) {
        if a[i] == b[i] {
            continue;
        }
        let (al, bl) = (a[i].is_alphabetic(), b[i].is_alphabetic());
        if al && bl {
            return a[i].cmp(&b[i]);
        }
        if al || bl {
            return al.cmp(&bl);
        }
        // A zero inside a number that began earlier still counts.
        let (mut an, mut bn) = (0u128, 0u128);
        if (a[i] == '0' || b[i] == '0') && a[..i].iter().rev().take_while(|c| c.is_ascii_digit()).any(|&c| c != '0') {
            (an, bn) = (1, 1);
        }
        let run = |s: &[char], mut n: u128| {
            let digits = s[i..].iter().take_while(|c| c.is_ascii_digit());
            let len = digits.clone().count();
            for c in digits {
                n = n.saturating_mul(10).saturating_add(*c as u128 - '0' as u128);
            }
            (n, len)
        };
        let ((an, alen), (bn, blen)) = (run(&a, an), run(&b, bn));
        return an.cmp(&bn).then(alen.cmp(&blen)).then(a[i].cmp(&b[i]));
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn like_kubectl() {
        let v = json!({
            "apiVersion": "v1",
            "metadata": { "resourceVersion": "749", "creationTimestamp": "2026-11-05T01:40:18Z",
                "annotations": { "a": "{\"x\":1}\n" }, "generateName": "hello-84b4bdb685-" },
            "spec": { "containers": [{ "command": ["sh", "-c", "echo hello from $(hostname); sleep 100000"],
                "name": "hello", "resources": {} }], "tolerations": [] },
            "status": { "conditions": [{ "lastProbeTime": null, "status": "True" }] },
            "x": ["- a", "*", "1.36", "a: b", "busybox:1.36", "10.0.0.1:6443"],
            "k": { "a19": 1, "a100": 2, "a9": 3, "_b": 4, "Z": 5 },
        });
        let want = "apiVersion: v1
k:
  _b: 4
  Z: 5
  a9: 3
  a19: 1
  a100: 2
metadata:
  annotations:
    a: |
      {\"x\":1}
  creationTimestamp: \"2026-11-05T01:40:18Z\"
  generateName: hello-84b4bdb685-
  resourceVersion: \"749\"
spec:
  containers:
  - command:
    - sh
    - -c
    - echo hello from $(hostname); sleep 100000
    name: hello
    resources: {}
  tolerations: []
status:
  conditions:
  - lastProbeTime: null
    status: \"True\"
x:
- '- a'
- '*'
- \"1.36\"
- 'a: b'
- busybox:1.36
- 10.0.0.1:6443
";
        assert_eq!(to_string(&v), want);
    }
}
