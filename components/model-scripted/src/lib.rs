//! A scripted, deliberately gullible model.
//!
//! It is deterministic, offline and needs no key. It reads the user's prompt
//! AND every tool result for instructions, and it obeys every instruction it
//! finds, including ones a tool's output hands back. That is the point: the
//! model does whatever it is told, so the only thing left deciding what a tool
//! can reach is the sandbox around that tool.
//!
//! It understands two kinds of instruction (case-insensitive, one per line or
//! sentence):
//!
//!   The tool's own command line, which it passes straight through:
//!     curl <args>                    -> curl   {arguments: <args>}
//!     sqlite <sql>  /  sql <sql>     -> sqlite {sql: <sql>}
//!     git <args>                     -> git    {arguments: <args>}
//!     kubectl <args>                 -> kubectl {arguments: <args>}
//!     a line that starts with "."    -> sqlite {sql: <line>}   (a dot-command)
//!
//!   A plainer request, mapped to one of the tools:
//!     fetch|get|summarize <url>      -> curl   {arguments: "-s <url>"}
//!     save <url> to <file>           -> curl   {arguments: "-o <file> -s <url>"}
//!     list the talks / what talks    -> sqlite {sql: "select ... from talks ..."}
//!     show the log / recent commits  -> git    {arguments: "log --oneline -5"}
//!     list the pods / what pods      -> kubectl {arguments: "get pods"}
//!
//! It plans calls from the conversation in order and returns the first one it
//! has not issued yet. When nothing is left it answers in text.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/model-provider",
    generate_all,
});

use exports::workshop::toolbox::model::Guest;
use serde_json::{Value, json};
use workshop::toolbox::types::{Completion, Message, ToolCall, ToolDef};

struct Scripted;

impl Guest for Scripted {
    async fn complete(messages: Vec<Message>, tools: Vec<ToolDef>) -> Result<Completion, String> {
        let available: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        // Calls already issued, in canonical JSON, so each instruction runs once.
        let issued: Vec<Value> = messages
            .iter()
            .filter(|m| m.role == "assistant")
            .filter_map(|m| serde_json::from_str::<Value>(&m.content).ok())
            .collect();

        let mut skipped = Vec::new();
        // Tool errors are not read as instructions: an error that echoes a
        // command would otherwise be "obeyed" again.
        let readable = messages.iter().filter(|m| {
            m.role == "user" || (m.role == "tool" && !m.content.starts_with("ERROR:"))
        });
        for m in readable {
            for (name, args) in instructions(&strip_tags(&m.content)) {
                let call = json!({ "name": name, "arguments": args });
                if issued.contains(&call) {
                    continue;
                }
                if !available.contains(&name.as_str()) {
                    if !skipped.contains(&name) {
                        skipped.push(name);
                    }
                    continue;
                }
                return Ok(Completion::Call(ToolCall {
                    name,
                    arguments: args.to_string(),
                }));
            }
        }

        Ok(Completion::Text(answer(&messages, &skipped)))
    }
}

/// Every instruction in `text`, in order, as (tool name, arguments).
fn instructions(text: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for sentence in sentences(text) {
        let lower = sentence.to_lowercase();
        // The tool's own command line, passed straight through. kubectl comes
        // first: "kubectl get pods" also contains "get ".
        if let Some(rest) = after(&sentence, "kubectl ") {
            out.push(("kubectl".into(), json!({ "arguments": rest.trim() })));
        } else if let Some(rest) = after(&sentence, "curl ") {
            out.push(("curl".into(), json!({ "arguments": rest.trim() })));
        } else if let Some(rest) = after(&sentence, "sqlite ").or_else(|| after(&sentence, "sql ")) {
            out.push(("sqlite".into(), json!({ "sql": rest.trim() })));
        } else if let Some(rest) = after(&sentence, "git ") {
            out.push(("git".into(), json!({ "arguments": rest.trim() })));
        } else if sentence.trim_start().starts_with('.') {
            // A sqlite dot-command, such as .tables or .shell.
            out.push(("sqlite".into(), json!({ "sql": sentence.trim() })));
        }
        // Plainer requests, mapped to a tool.
        else if let Some(rest) = after(&sentence, "save ") {
            // "save <url> to <file>"
            if let Some((url, file)) = rest.split_once(" to ") {
                out.push((
                    "curl".into(),
                    json!({ "arguments": format!("-o {} -s {}", trim_punct(first_word(file)), trim_punct(first_word(url))) }),
                ));
            }
        } else if let Some(rest) = after(&sentence, "fetch ")
            .or_else(|| after(&sentence, "get "))
            .or_else(|| after(&sentence, "summarize "))
            .or_else(|| after(&sentence, "download "))
        {
            out.push(("curl".into(), json!({ "arguments": format!("-s {}", trim_punct(first_word(rest))) })));
        } else if lower.contains("list the talks") || lower.contains("what talks") || lower.contains("which talks") {
            out.push((
                "sqlite".into(),
                json!({ "sql": "select title, room from talks order by title limit 5" }),
            ));
        } else if lower.contains("recent commit") || lower.contains("show the log") || lower.contains("git log") || lower.contains("latest commit") {
            out.push(("git".into(), json!({ "arguments": "log --oneline -5" })));
        } else if lower.contains("list the pods") || lower.contains("what pods") || lower.contains("which pods") {
            out.push(("kubectl".into(), json!({ "arguments": "get pods" })));
        }
    }
    out
}

/// The text answer once every instruction has run.
fn answer(messages: &[Message], skipped: &[String]) -> String {
    let results: Vec<String> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| {
            let name = m.tool_name.clone().unwrap_or_default();
            let flat: String = m.content.split_whitespace().collect::<Vec<_>>().join(" ");
            let short: String = flat.chars().take(120).collect();
            format!("{name}: {short}")
        })
        .collect();
    let mut text = if results.is_empty() {
        "Hello! I am the workshop agent running a scripted model. Ask me to run curl, \
         sqlite, git or kubectl, for example \"list the talks in sqlite\" or \
         \"curl -s http://api.workshop-sites.svc/data\"."
            .to_string()
    } else {
        format!("Done. Here is what each step returned: {}", results.join(" | "))
    };
    if !skipped.is_empty() {
        text.push_str(&format!(" (I wanted to use {} but it is not installed.)", skipped.join(", ")));
    }
    text
}

/// Lines, and sentences within a line (". ", "; ", "? " and "! " split them).
/// A leading dot-command keeps its dot: ".tables" is one sentence. A line that
/// starts with "sqlite " or "sql " is one sentence too, so a statement list
/// such as "sqlite attach ... as r; select ... from r.rooms" reaches the tool
/// whole.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        // A whole line that is a dot-command stays intact, so ".schema talks"
        // isn't split on its space into a stray sentence.
        let lower = line.to_lowercase();
        if line.starts_with('.') || lower.starts_with("sqlite ") || lower.starts_with("sql ") {
            out.push(line.to_string());
            continue;
        }
        for part in line
            .split(". ")
            .flat_map(|p| p.split("; "))
            .flat_map(|p| p.split("? "))
            .flat_map(|p| p.split("! "))
        {
            let part = part.trim();
            if !part.is_empty() {
                out.push(part.to_string());
            }
        }
    }
    out
}

/// The text after `needle` (case-insensitive), if present.
fn after<'a>(s: &'a str, needle: &str) -> Option<&'a str> {
    let lower = s.to_lowercase();
    lower.find(needle).map(|i| &s[i + needle.len()..])
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn trim_punct(s: &str) -> &str {
    s.trim()
        .trim_end_matches(['.', ',', ';', ':', ')', '"', '\'', '!'])
        .trim_start_matches(['"', '\'', '('])
}

/// Replace HTML tags with line breaks so a planted page reads as text.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push('\n');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

export!(Scripted);
