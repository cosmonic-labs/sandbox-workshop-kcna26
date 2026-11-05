// Vendored from cosmonic-labs/agent-tools (crates/tool-common/src/args.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Unmodified.

//! Strict parsing of a model's JSON arguments.
//!
//! Every tool refuses unknown keys and wrong types with a message that names
//! the key and what was expected, so the model can correct itself. A refusal
//! is [`CallError::NotDispatched`] with `invalid-arguments`: nothing ran.

use serde_json::{Map, Value};

use crate::types::CallError;

/// A parsed arguments object.
#[derive(Debug)]
pub struct Args {
    tool: &'static str,
    map: Map<String, Value>,
}

impl Args {
    /// Parse `json` as an object whose keys are all in `allowed`. An empty
    /// string or `null` is an empty object.
    pub fn parse(tool: &'static str, json: &str, allowed: &[&str]) -> Result<Self, CallError> {
        let trimmed = json.trim();
        let value: Value = if trimmed.is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(trimmed).map_err(|e| {
                CallError::invalid(format!("{tool}: arguments are not valid JSON: {e}"))
            })?
        };
        let map = match value {
            Value::Object(m) => m,
            Value::Null => Map::new(),
            other => {
                return Err(CallError::invalid(format!(
                    "{tool}: arguments must be a JSON object, got {}",
                    kind(&other)
                )))
            }
        };
        let mut unknown: Vec<&str> = map
            .keys()
            .map(String::as_str)
            .filter(|k| !allowed.contains(k))
            .collect();
        if !unknown.is_empty() {
            unknown.sort_unstable();
            return Err(CallError::invalid(format!(
                "{tool}: unknown argument{} {}; allowed: {}",
                if unknown.len() == 1 { "" } else { "s" },
                unknown.join(", "),
                allowed.join(", ")
            )));
        }
        Ok(Self { tool, map })
    }

    fn bad(&self, key: &str, want: &str, got: &Value) -> CallError {
        CallError::invalid(format!(
            "{}: `{key}` must be {want}, got {}",
            self.tool,
            kind(got)
        ))
    }

    /// An optional string. `null` counts as absent; an empty string is kept.
    pub fn opt_str(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(v) => Err(self.bad(key, "a string", v)),
        }
    }

    /// An optional, non-empty string.
    pub fn opt_nonempty(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.opt_str(key)? {
            Some(s) if s.trim().is_empty() => Err(CallError::invalid(format!(
                "{}: `{key}` must not be empty",
                self.tool
            ))),
            other => Ok(other),
        }
    }

    /// A required, non-empty string.
    pub fn req_str(&self, key: &str) -> Result<String, CallError> {
        self.opt_nonempty(key)?
            .ok_or_else(|| CallError::invalid(format!("{}: `{key}` is required", self.tool)))
    }

    pub fn opt_bool(&self, key: &str) -> Result<Option<bool>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(v) => Err(self.bad(key, "true or false", v)),
        }
    }

    pub fn bool_or(&self, key: &str, default: bool) -> Result<bool, CallError> {
        Ok(self.opt_bool(key)?.unwrap_or(default))
    }

    /// An optional integer in `min..=max`. A whole-number float (`20.0`) is
    /// accepted, since some models write numbers that way.
    pub fn opt_u64(&self, key: &str, min: u64, max: u64) -> Result<Option<u64>, CallError> {
        let v = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(None),
            Some(v) => v,
        };
        let n = v
            .as_u64()
            .or_else(|| {
                v.as_f64()
                    .filter(|f| f.fract() == 0.0 && *f >= 0.0 && *f <= u64::MAX as f64)
                    .map(|f| f as u64)
            })
            .ok_or_else(|| self.bad(key, "a whole number", v))?;
        if n < min || n > max {
            return Err(CallError::invalid(format!(
                "{}: `{key}` must be between {min} and {max}, got {n}",
                self.tool
            )));
        }
        Ok(Some(n))
    }

    pub fn u64_or(&self, key: &str, default: u64, min: u64, max: u64) -> Result<u64, CallError> {
        Ok(self.opt_u64(key, min, max)?.unwrap_or(default))
    }

    /// An optional number in `min..=max`.
    pub fn opt_f64(&self, key: &str, min: f64, max: f64) -> Result<Option<f64>, CallError> {
        let v = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(None),
            Some(v) => v,
        };
        // Some models send numbers as strings; accept "51.5" too.
        let n = v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
            .filter(|n| n.is_finite())
            .ok_or_else(|| self.bad(key, "a number", v))?;
        if n < min || n > max {
            return Err(CallError::invalid(format!(
                "{}: `{key}` must be between {min} and {max}, got {n}",
                self.tool
            )));
        }
        Ok(Some(n))
    }

    /// A required number in `min..=max`.
    pub fn req_f64(&self, key: &str, min: f64, max: f64) -> Result<f64, CallError> {
        self.opt_f64(key, min, max)?
            .ok_or_else(|| CallError::invalid(format!("{}: `{key}` is required", self.tool)))
    }

    /// An optional list of strings, given as a JSON array or as one
    /// comma-separated string. At most `max` items.
    pub fn opt_str_list(&self, key: &str, max: usize) -> Result<Option<Vec<String>>, CallError> {
        let items: Vec<String> = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(s)) => s
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.trim().to_string())
                        .ok_or_else(|| self.bad(key, "a list of strings", v))
                })
                .collect::<Result<_, _>>()?,
            Some(v) => return Err(self.bad(key, "a list of strings", v)),
        };
        if items.len() > max {
            return Err(CallError::invalid(format!(
                "{}: `{key}` takes at most {max} items, got {}",
                self.tool,
                items.len()
            )));
        }
        Ok(Some(items))
    }

    /// An optional string that must be one of `choices`.
    pub fn opt_choice(&self, key: &str, choices: &[&str]) -> Result<Option<String>, CallError> {
        match self.opt_str(key)? {
            None => Ok(None),
            Some(s) if choices.contains(&s.as_str()) => Ok(Some(s)),
            Some(s) => Err(CallError::invalid(format!(
                "{}: `{key}` must be one of {}, got {s:?}",
                self.tool,
                choices.join(", ")
            ))),
        }
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Dispatch;

    fn msg(e: CallError) -> String {
        match e {
            CallError::NotDispatched(Dispatch::InvalidArguments(m)) => m,
            other => panic!("expected invalid-arguments, got {other:?}"),
        }
    }

    #[test]
    fn empty_and_null_are_empty_objects() {
        for json in ["", "  ", "null", "{}"] {
            let a = Args::parse("t", json, &["x"]).unwrap();
            assert_eq!(a.opt_str("x").unwrap(), None);
        }
    }

    #[test]
    fn refuses_non_objects_and_bad_json() {
        assert!(msg(Args::parse("t", "[1]", &[]).unwrap_err()).contains("an array"));
        assert!(msg(Args::parse("t", "{", &[]).unwrap_err()).contains("not valid JSON"));
    }

    #[test]
    fn refuses_unknown_keys_naming_them() {
        let m = msg(Args::parse("t", r#"{"b":1,"a":2,"ok":3}"#, &["ok"]).unwrap_err());
        assert!(m.contains("unknown arguments a, b"), "{m}");
        assert!(m.contains("allowed: ok"), "{m}");
    }

    #[test]
    fn typed_getters() {
        let a = Args::parse(
            "t",
            r#"{"s":"x","n":5,"f":7.0,"b":true,"e":"","c":"open","neg":-1}"#,
            &["s", "n", "f", "b", "e", "c", "neg", "missing"],
        )
        .unwrap();
        assert_eq!(a.req_str("s").unwrap(), "x");
        assert_eq!(a.u64_or("n", 1, 1, 10).unwrap(), 5);
        assert_eq!(a.u64_or("f", 1, 1, 10).unwrap(), 7);
        assert_eq!(a.u64_or("missing", 3, 1, 10).unwrap(), 3);
        assert!(a.bool_or("b", false).unwrap());
        assert!(msg(a.req_str("e").unwrap_err()).contains("must not be empty"));
        assert!(msg(a.req_str("missing").unwrap_err()).contains("is required"));
        assert!(msg(a.u64_or("n", 1, 1, 4).unwrap_err()).contains("between 1 and 4"));
        assert!(msg(a.opt_u64("neg", 0, 9).unwrap_err()).contains("whole number"));
        assert!(msg(a.opt_bool("s").unwrap_err()).contains("true or false"));
        assert_eq!(
            a.opt_choice("c", &["open", "closed"]).unwrap().as_deref(),
            Some("open")
        );
        assert!(msg(a.opt_choice("s", &["open"]).unwrap_err()).contains("one of open"));
    }

    #[test]
    fn string_lists() {
        let a = Args::parse(
            "t",
            r#"{"a":["USD"," gbp "],"s":"USD, JPY,","n":[1],"x":3}"#,
            &["a", "s", "n", "x"],
        )
        .unwrap();
        assert_eq!(a.opt_str_list("a", 5).unwrap().unwrap(), ["USD", "gbp"]);
        assert_eq!(a.opt_str_list("s", 5).unwrap().unwrap(), ["USD", "JPY"]);
        assert!(msg(a.opt_str_list("s", 1).unwrap_err()).contains("at most 1"));
        assert!(msg(a.opt_str_list("n", 5).unwrap_err()).contains("list of strings"));
        assert!(msg(a.opt_str_list("x", 5).unwrap_err()).contains("list of strings"));
        assert_eq!(a.opt_str_list("missing", 5).unwrap(), None);
    }

    #[test]
    fn floats() {
        let a = Args::parse(
            "t",
            r#"{"lat":51.5,"lon":"-0.12","bad":"x","big":200}"#,
            &["lat", "lon", "bad", "big"],
        )
        .unwrap();
        assert_eq!(a.req_f64("lat", -90.0, 90.0).unwrap(), 51.5);
        assert_eq!(a.opt_f64("lon", -180.0, 180.0).unwrap(), Some(-0.12));
        assert!(msg(a.opt_f64("bad", 0.0, 1.0).unwrap_err()).contains("a number"));
        assert!(msg(a.opt_f64("big", -90.0, 90.0).unwrap_err()).contains("between"));
        assert!(msg(a.req_f64("none", 0.0, 1.0).unwrap_err()).contains("is required"));
    }
}
