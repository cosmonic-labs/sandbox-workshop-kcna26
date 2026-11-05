//! gh's command line, parsed. No I/O here, so it tests natively:
//! `cargo test -p tool-gh` on your own machine.
//!
//! Each read command becomes one of github-core's six tools and the JSON
//! arguments it takes, so the requests and the output are github-core's. The
//! flags follow GitHub's own `gh` where the two overlap.

/// The repository a command reads when it names none with -R.
pub const DEFAULT_REPO: &str = "wasmCloud/wasmCloud";

/// What a gh command line asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// One of github-core's tools, with its JSON arguments.
    Tool { name: &'static str, arguments: String },
    /// `gh api`: one request of your own, sent through the capability as is.
    Api { method: String, path: String },
}

/// Why a command line was not accepted.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    /// Malformed, or a command this tool doesn't know.
    Invalid(String),
    /// A command this tool doesn't offer because it changes GitHub.
    Writes(String),
}

const USAGE: &str = "this gh offers: pr list, pr view [--comments], pr checks, pr diff, \
                     search prs, and api (GET)";

/// Commands that change GitHub. This gh only reads, so it offers none of them.
const WRITES: &[&str] = &[
    "pr checkout", "pr close", "pr comment", "pr create", "pr edit", "pr lock", "pr merge",
    "pr ready", "pr reopen", "pr review", "pr revert", "pr unlock", "pr update-branch",
    "issue close", "issue comment", "issue create", "issue delete", "issue edit",
    "repo create", "repo delete", "repo edit", "repo fork", "release create",
    "release delete", "auth login", "auth token", "secret set", "workflow run",
];

/// Split a command line the way a shell would: on whitespace, keeping
/// single- and double-quoted parts together.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => cur.extend(chars.next()),
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                cur.extend(chars.next());
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unmatched quote".into());
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

/// The words of a command after its name, split into positional arguments
/// and flags. A flag takes the next word as its value unless it is one of
/// `switches`; `--flag=value` works too.
struct Parsed {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Parsed {
    fn new(words: &[String], switches: &[&str]) -> Result<Self, Refusal> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let w = &words[i];
            if w.starts_with('-') && w.len() > 1 && w.parse::<i64>().is_err() {
                if let Some((k, v)) = w.split_once('=') {
                    flags.push((k.to_string(), Some(v.to_string())));
                } else if switches.contains(&w.as_str()) {
                    flags.push((w.clone(), None));
                } else {
                    let v = words
                        .get(i + 1)
                        .ok_or_else(|| Refusal::Invalid(format!("flag needs an argument: {w}")))?;
                    flags.push((w.clone(), Some(v.clone())));
                    i += 1;
                }
            } else {
                positional.push(w.clone());
            }
            i += 1;
        }
        Ok(Self { positional, flags })
    }

    /// The value of the first of `names` given.
    fn value(&self, names: &[&str]) -> Option<&str> {
        self.flags
            .iter()
            .find(|(k, _)| names.contains(&k.as_str()))
            .and_then(|(_, v)| v.as_deref())
    }

    fn on(&self, names: &[&str]) -> bool {
        self.flags.iter().any(|(k, _)| names.contains(&k.as_str()))
    }

    fn only(&self, known: &[&str]) -> Result<(), Refusal> {
        match self.flags.iter().find(|(k, _)| !known.contains(&k.as_str())) {
            Some((k, _)) => Err(Refusal::Invalid(format!("unknown flag: {k}"))),
            None => Ok(()),
        }
    }

    fn repo(&self) -> String {
        self.value(&["-R", "--repo"]).unwrap_or(DEFAULT_REPO).to_string()
    }

    fn number(&self, what: &str) -> Result<u64, Refusal> {
        let n = self
            .positional
            .first()
            .ok_or_else(|| Refusal::Invalid(format!("{what}: give a pull request number")))?;
        // A number, #number, or the pull request's URL.
        let tail = n.trim_end_matches('/').rsplit('/').next().unwrap_or(n);
        tail.trim_start_matches('#')
            .parse::<u64>()
            .map_err(|_| Refusal::Invalid(format!("{what}: not a pull request number: {n}")))
    }

    fn limit(&self, max: u64) -> Result<Option<u64>, Refusal> {
        match self.value(&["-L", "--limit"]) {
            None => Ok(None),
            Some(v) => match v.parse::<u64>() {
                Ok(n) if n >= 1 => Ok(Some(n.min(max))),
                _ => Err(Refusal::Invalid(format!("invalid value for --limit: {v}"))),
            },
        }
    }
}

fn tool(name: &'static str, arguments: serde_json::Value) -> Result<Command, Refusal> {
    Ok(Command::Tool { name, arguments: arguments.to_string() })
}

/// Parse what was typed after `gh`.
pub fn parse(line: &str) -> Result<Command, Refusal> {
    let words = split(line).map_err(Refusal::Invalid)?;
    let words: Vec<String> = match words.first().map(String::as_str) {
        Some("gh") => words[1..].to_vec(),
        _ => words,
    };
    let (group, verb) = (
        words.first().map(String::as_str).unwrap_or(""),
        words.get(1).map(String::as_str).unwrap_or(""),
    );
    let pair = format!("{group} {verb}");
    if WRITES.contains(&pair.as_str()) {
        return Err(Refusal::Writes(format!(
            "'gh {pair}' changes GitHub; this gh only reads ({USAGE})"
        )));
    }
    let rest = words.get(2..).unwrap_or(&[]);
    match (group, verb) {
        ("pr", "list") => {
            let p = Parsed::new(rest, &[])?;
            p.only(&["-R", "--repo", "-s", "--state", "-L", "--limit", "-S", "--search", "-B", "--base"])?;
            let state = p.value(&["-s", "--state"]).unwrap_or("open");
            if !["open", "closed", "merged", "all"].contains(&state) {
                return Err(Refusal::Invalid(format!(
                    "invalid value for --state: {state} (open, closed, merged or all)"
                )));
            }
            let limit = p.limit(50)?.unwrap_or(30);
            let search = p.value(&["-S", "--search"]);
            // gh lists merged pull requests, and --search, through search.
            if search.is_some() || state == "merged" {
                let mut q = search.unwrap_or("").to_string();
                if state == "merged" {
                    q = format!("{q} is:merged").trim().to_string();
                }
                if let Some(b) = p.value(&["-B", "--base"]) {
                    q.push_str(&format!(" base:{b}"));
                }
                let mut args = serde_json::json!({ "query": q, "repo": p.repo(), "max_count": limit, "sort": "updated" });
                if state == "open" || state == "closed" {
                    args["state"] = state.into();
                }
                return tool("gh_search_prs", args);
            }
            let mut args = serde_json::json!({ "repo": p.repo(), "state": state, "max_count": limit });
            if let Some(b) = p.value(&["-B", "--base"]) {
                args["base"] = b.into();
            }
            tool("gh_list_prs", args)
        }
        ("pr", "view") => {
            let p = Parsed::new(rest, &["-c", "--comments"])?;
            p.only(&["-R", "--repo", "-c", "--comments"])?;
            let n = p.number("pr view")?;
            if p.on(&["-c", "--comments"]) {
                tool("gh_pr_comments", serde_json::json!({ "repo": p.repo(), "number": n }))
            } else {
                tool("gh_get_pr", serde_json::json!({ "repo": p.repo(), "number": n }))
            }
        }
        ("pr", "checks") => {
            let p = Parsed::new(rest, &[])?;
            p.only(&["-R", "--repo"])?;
            let n = p.number("pr checks")?;
            tool("gh_pr_checks", serde_json::json!({ "repo": p.repo(), "number": n }))
        }
        ("pr", "diff") => {
            let p = Parsed::new(rest, &["--name-only"])?;
            p.only(&["-R", "--repo", "--name-only"])?;
            let n = p.number("pr diff")?;
            let patches = !p.on(&["--name-only"]);
            tool(
                "gh_pr_files",
                serde_json::json!({ "repo": p.repo(), "number": n, "patches": patches, "max_patch_bytes": 2048 }),
            )
        }
        ("search", "prs") => {
            let p = Parsed::new(rest, &[])?;
            p.only(&["-R", "--repo", "--state", "-L", "--limit"])?;
            let q = p.positional.join(" ");
            if q.trim().is_empty() {
                return Err(Refusal::Invalid("search prs: give a search query".into()));
            }
            let mut args = serde_json::json!({ "query": q, "max_count": p.limit(50)?.unwrap_or(30) });
            if let Some(r) = p.value(&["-R", "--repo"]) {
                args["repo"] = r.into();
            }
            if let Some(s) = p.value(&["--state"]) {
                args["state"] = s.into();
            }
            tool("gh_search_prs", args)
        }
        ("api", _) => {
            let p = Parsed::new(&words[1..], &[])?;
            p.only(&["-X", "--method"])?;
            let method = p.value(&["-X", "--method"]).unwrap_or("GET").to_ascii_uppercase();
            let path = match p.positional.as_slice() {
                [one] => one.clone(),
                _ => return Err(Refusal::Invalid("api: give one path, such as /rate_limit".into())),
            };
            // gh accepts the path without its leading slash.
            let path = if path.starts_with('/') { path } else { format!("/{path}") };
            Ok(Command::Api { method, path })
        }
        ("", _) => Err(Refusal::Invalid(format!("no command given; {USAGE}"))),
        _ => Err(Refusal::Invalid(format!("unknown command 'gh {}'; {USAGE}", pair.trim()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_of(line: &str) -> (&'static str, serde_json::Value) {
        match parse(line).unwrap() {
            Command::Tool { name, arguments } => (name, serde_json::from_str(&arguments).unwrap()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn lists_with_gh_flags() {
        let (n, a) = tool_of("pr list -R wasmCloud/wasmCloud --limit 5");
        assert_eq!(n, "gh_list_prs");
        assert_eq!(a["repo"], "wasmCloud/wasmCloud");
        assert_eq!(a["max_count"], 5);
        assert_eq!(a["state"], "open");
        let (n, a) = tool_of("gh pr list -S 'label:bug' -s closed");
        assert_eq!(n, "gh_search_prs");
        assert_eq!(a["query"], "label:bug");
        assert_eq!(a["state"], "closed");
        let (n, a) = tool_of("pr list --state merged -L 3");
        assert_eq!(n, "gh_search_prs");
        assert_eq!(a["query"], "is:merged");
        assert_eq!(a["repo"], DEFAULT_REPO);
    }

    #[test]
    fn views_checks_diffs() {
        assert_eq!(tool_of("pr view 4321").0, "gh_get_pr");
        assert_eq!(tool_of("pr view #4321 --comments").0, "gh_pr_comments");
        assert_eq!(tool_of("pr view https://github.com/a/b/pull/7").1["number"], 7);
        assert_eq!(tool_of("pr checks 12 -R a/b").1["repo"], "a/b");
        assert_eq!(tool_of("pr diff 12 --name-only").1["patches"], false);
        assert_eq!(tool_of("search prs is:open author:octocat --limit 2").1["query"], "is:open author:octocat");
    }

    #[test]
    fn api_and_refusals() {
        assert_eq!(
            parse("api -X POST /repos/a/b/issues/1/comments").unwrap(),
            Command::Api { method: "POST".into(), path: "/repos/a/b/issues/1/comments".into() }
        );
        assert_eq!(parse("api rate_limit").unwrap(), Command::Api { method: "GET".into(), path: "/rate_limit".into() });
        assert!(matches!(parse("pr merge 12"), Err(Refusal::Writes(_))));
        assert!(matches!(parse("pr list --bogus 1"), Err(Refusal::Invalid(_))));
        assert!(matches!(parse("pr view"), Err(Refusal::Invalid(_))));
        assert!(matches!(parse("repo view"), Err(Refusal::Invalid(_))));
    }
}
