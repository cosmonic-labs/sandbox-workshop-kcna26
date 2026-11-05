// Vendored from cosmonic-labs/agent-tools (crates/github-core/src/util.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Unmodified.

//! Small helpers: query encoding, UTC times, argument checks.

use tool_common::CallError;

/// Percent-encode a query value (RFC 3986 unreserved characters kept).
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `owner/name`, checked against GitHub's rules for each part.
pub fn repo(s: &str) -> Result<(String, String), CallError> {
    let bad = || {
        CallError::invalid(format!(
            "`repo` must be owner/name, like wasmCloud/wasmCloud, got {s:?}"
        ))
    };
    let s = s.trim().trim_start_matches("https://github.com/");
    let s = s.trim_end_matches('/').trim_end_matches(".git");
    let (owner, name) = s.split_once('/').ok_or_else(bad)?;
    let owner_ok = !owner.is_empty()
        && owner.len() <= 39
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !owner.starts_with('-');
    let name_ok = !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if owner_ok && name_ok {
        Ok((owner.to_string(), name.to_string()))
    } else {
        Err(bad())
    }
}

/// `YYYY-MM-DD HH:MM` (UTC) for seconds since the epoch.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "in 12 minutes", "in 45 seconds", "now".
pub fn until(now: u64, then: u64) -> String {
    if then <= now {
        return "now".into();
    }
    let s = then - now;
    if s < 90 {
        format!("in {s} seconds")
    } else {
        format!("in {} minutes", s.div_ceil(60))
    }
}

/// The date part of a GitHub timestamp (`2026-10-01T12:00:00Z`).
pub fn day(ts: &str) -> &str {
    ts.get(..10).unwrap_or(ts)
}

/// A JSON string field, or "".
pub fn s<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// A user object's login, or "ghost" for a deleted account.
pub fn login(v: &serde_json::Value) -> &str {
    v.get("login").and_then(|x| x.as_str()).unwrap_or("ghost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query_values() {
        assert_eq!(enc("is:pr repo:a/b"), "is%3Apr%20repo%3Aa%2Fb");
        assert_eq!(enc("ok-._~"), "ok-._~");
        assert_eq!(enc("é"), "%C3%A9");
    }

    #[test]
    fn checks_repo_names() {
        assert_eq!(
            repo("wasmCloud/wasmCloud").unwrap(),
            ("wasmCloud".into(), "wasmCloud".into())
        );
        assert_eq!(
            repo("https://github.com/bytecodealliance/wasmtime.git")
                .unwrap()
                .1,
            "wasmtime"
        );
        for bad in ["", "a", "a/b/c", "a/..", "-a/b", "a b/c", "a/b?x=1", "../x"] {
            assert!(repo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn utc_times() {
        assert_eq!(utc(0), "1970-01-01 00:00");
        assert_eq!(utc(1_792_000_000), "2026-10-14 17:46");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00");
        assert_eq!(until(100, 100), "now");
        assert_eq!(until(100, 130), "in 30 seconds");
        assert_eq!(until(0, 600), "in 10 minutes");
        assert_eq!(until(0, 601), "in 11 minutes");
    }
}
