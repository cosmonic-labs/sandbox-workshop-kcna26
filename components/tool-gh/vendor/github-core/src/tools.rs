// Vendored from cosmonic-labs/agent-tools (crates/github-core/src/tools.rs, commit aef5ffc),
// Apache-2.0, Copyright 2026 Cosmonic, Inc. See ../../LICENSE and ../../NOTICE.
// Unmodified.

//! The six tools.

use serde_json::{json, Value};
use tool_common::args::Args;
use tool_common::bound::{clip, structured, Budget};
use tool_common::{CallError, Reply};

use crate::client::{finish, Client, Fail};
use crate::util::{day, enc, login, repo, s};
use tool_common::http::Http;

fn labels(pr: &Value) -> Vec<String> {
    pr.get("labels")
        .and_then(|l| l.as_array())
        .map(|a| a.iter().map(|l| s(l, "name").to_string()).collect())
        .unwrap_or_default()
}

fn one_line(t: &str, max: usize) -> String {
    let line = t.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        let mut o: String = line.chars().take(max - 1).collect();
        o.push('…');
        o
    }
}

/// Finish a result: the rate note goes last, inside the budget's reserve.
fn done<H: Http>(c: &Client<'_, H>, mut out: Budget, value: Value, small: Value) -> Reply {
    let note = c.rate_note();
    if !out.push_whole(&format!("\n{note}\n")) {
        out.set_note(format!("[output limit reached] {note}"));
    }
    let mut value = value;
    if let (Some(obj), Some(r)) = (value.as_object_mut(), c.rate()) {
        obj.insert(
            "rate_limit".into(),
            json!({ "remaining": r.remaining, "limit": r.limit, "reset": r.reset, "resource": r.resource }),
        );
    }
    let text = out.finish(|| note.clone());
    Reply::ok(text, structured(&value, small))
}

fn pr_number(a: &Args) -> Result<u64, CallError> {
    a.opt_u64("number", 1, 100_000_000)?
        .ok_or_else(|| CallError::invalid("`number` is required: the pull request number"))
}

fn pr_line(pr: &Value) -> String {
    let mut flags = Vec::new();
    if pr.get("draft").and_then(|d| d.as_bool()) == Some(true) {
        flags.push("draft".to_string());
    }
    if s(pr, "state") == "closed" {
        flags.push(if pr.get("merged_at").is_some_and(|m| !m.is_null()) {
            "merged".into()
        } else {
            "closed".into()
        });
    }
    let l = labels(pr);
    if !l.is_empty() {
        flags.push(format!("labels: {}", l.join(", ")));
    }
    let reviewers: Vec<&str> = pr
        .get("requested_reviewers")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().map(login).collect())
        .unwrap_or_default();
    if !reviewers.is_empty() {
        flags.push(format!("review requested: {}", reviewers.join(", ")));
    }
    format!(
        "#{} {} (by {}, {} <- {}, updated {}){}\n",
        pr.get("number").and_then(|n| n.as_u64()).unwrap_or(0),
        one_line(s(pr, "title"), 120),
        login(&pr["user"]),
        s(&pr["base"], "ref"),
        s(&pr["head"], "label"),
        day(s(pr, "updated_at")),
        if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join("; "))
        }
    )
}

pub async fn list_prs<H: Http>(c: &Client<'_, H>, arguments: &str) -> Result<Reply, CallError> {
    let a = Args::parse(
        "gh_list_prs",
        arguments,
        &[
            "repo",
            "state",
            "sort",
            "direction",
            "base",
            "max_count",
            "page",
        ],
    )?;
    let (owner, name) = repo(&a.req_str("repo")?)?;
    let state = a
        .opt_choice("state", &["open", "closed", "all"])?
        .unwrap_or_else(|| "open".into());
    let sort = a
        .opt_choice(
            "sort",
            &["created", "updated", "popularity", "long-running"],
        )?
        .unwrap_or_else(|| "updated".into());
    let direction = a
        .opt_choice("direction", &["asc", "desc"])?
        .unwrap_or_else(|| "desc".into());
    let base = a.opt_nonempty("base")?;
    let max = a.u64_or("max_count", 20, 1, 50)?;
    let page = a.u64_or("page", 1, 1, 100)?;
    finish(
        async {
            let mut q = format!(
                "/repos/{owner}/{name}/pulls?state={state}&sort={sort}&direction={direction}&per_page={max}&page={page}"
            );
            if let Some(b) = &base {
                q.push_str(&format!("&base={}", enc(b)));
            }
            let v = c.get_json(&q, &format!("the repository {owner}/{name}")).await?;
            let prs = v.as_array().cloned().unwrap_or_default();
            let mut out = Budget::default();
            out.push(&format!(
                "{} {state} pull request{} in {owner}/{name}, by {sort} ({direction}), page {page}:\n",
                prs.len(),
                if prs.len() == 1 { "" } else { "s" }
            ));
            let mut rows = Vec::new();
            for pr in &prs {
                if !out.push(&pr_line(pr)) {
                    break;
                }
                rows.push(json!({
                    "number": pr["number"], "title": s(pr, "title"), "author": login(&pr["user"]),
                    "draft": pr["draft"], "updated_at": s(pr, "updated_at"), "labels": labels(pr),
                }));
            }
            if prs.len() as u64 == max {
                out.push(&format!(
                    "[there may be more; call again with page={}]\n",
                    page + 1
                ));
            }
            Ok(done(
                c,
                out,
                json!({ "repo": format!("{owner}/{name}"), "pull_requests": rows }),
                json!({ "count": prs.len() }),
            ))
        }
        .await,
    )
}

pub async fn get_pr<H: Http>(c: &Client<'_, H>, arguments: &str) -> Result<Reply, CallError> {
    let a = Args::parse("gh_get_pr", arguments, &["repo", "number", "reviews"])?;
    let (owner, name) = repo(&a.req_str("repo")?)?;
    let number = pr_number(&a)?;
    let want_reviews = a.bool_or("reviews", true)?;
    finish(
        async {
            let what = format!("pull request #{number} in {owner}/{name}");
            let pr = c
                .get_json(&format!("/repos/{owner}/{name}/pulls/{number}"), &what)
                .await?;
            let mut out = Budget::default();
            let state = if pr.get("merged").and_then(|m| m.as_bool()) == Some(true) {
                "merged"
            } else {
                s(&pr, "state")
            };
            let draft = pr.get("draft").and_then(|d| d.as_bool()) == Some(true);
            out.push(&format!(
                "#{number} {}\n{}\nState: {state}{}\n",
                s(&pr, "title"),
                s(&pr, "html_url"),
                if draft { " (draft)" } else { "" }
            ));
            out.push(&format!(
                "Author: {} | created {} | updated {}\n",
                login(&pr["user"]),
                s(&pr, "created_at"),
                s(&pr, "updated_at")
            ));
            out.push(&format!(
                "Branches: {} <- {} (head {})\n",
                s(&pr["base"], "ref"),
                s(&pr["head"], "label"),
                s(&pr["head"], "sha").get(..8).unwrap_or("")
            ));
            let mergeable = match pr.get("mergeable") {
                Some(Value::Bool(true)) => "yes".to_string(),
                Some(Value::Bool(false)) => "no (conflicts)".to_string(),
                _ => "not computed yet (ask again shortly)".to_string(),
            };
            out.push(&format!(
                "Mergeable: {mergeable}; mergeable state: {}\n",
                pr.get("mergeable_state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            ));
            let n = |k: &str| pr.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
            out.push(&format!(
                "Size: {} commits, {} files, +{} -{}; {} comments, {} review comments\n",
                n("commits"),
                n("changed_files"),
                n("additions"),
                n("deletions"),
                n("comments"),
                n("review_comments")
            ));
            let l = labels(&pr);
            out.push(&format!(
                "Labels: {}\n",
                if l.is_empty() {
                    "none".into()
                } else {
                    l.join(", ")
                }
            ));
            let reviewers: Vec<String> = pr
                .get("requested_reviewers")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().map(|u| login(u).to_string()).collect())
                .unwrap_or_default();
            let teams: Vec<String> = pr
                .get("requested_teams")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().map(|t| format!("team {}", s(t, "slug"))).collect())
                .unwrap_or_default();
            let requested: Vec<String> = reviewers.iter().cloned().chain(teams).collect();
            out.push(&format!(
                "Review requested from: {}\n",
                if requested.is_empty() {
                    "nobody".into()
                } else {
                    requested.join(", ")
                }
            ));
            let assignees: Vec<String> = pr
                .get("assignees")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().map(|u| login(u).to_string()).collect())
                .unwrap_or_default();
            if !assignees.is_empty() {
                out.push(&format!("Assignees: {}\n", assignees.join(", ")));
            }
            let mut decisions = Vec::new();
            if want_reviews {
                let reviews = c
                    .get_json(
                        &format!("/repos/{owner}/{name}/pulls/{number}/reviews?per_page=100"),
                        &what,
                    )
                    .await?;
                // The latest decision per reviewer, as GitHub's UI shows it.
                let mut latest: Vec<(String, String, String)> = Vec::new();
                for r in reviews.as_array().into_iter().flatten() {
                    let who = login(&r["user"]).to_string();
                    let st = s(r, "state").to_string();
                    if st == "COMMENTED" || st == "PENDING" {
                        if !latest.iter().any(|(w, _, _)| *w == who) {
                            latest.push((who, st, s(r, "submitted_at").to_string()));
                        }
                        continue;
                    }
                    latest.retain(|(w, _, _)| *w != who);
                    latest.push((who, st, s(r, "submitted_at").to_string()));
                }
                if latest.is_empty() {
                    out.push("Reviews: none submitted\n");
                } else {
                    let parts: Vec<String> = latest
                        .iter()
                        .map(|(w, st, at)| {
                            format!("{w}: {} ({})", st.to_lowercase().replace('_', " "), day(at))
                        })
                        .collect();
                    out.push(&format!("Reviews: {}\n", parts.join(", ")));
                }
                decisions = latest
                    .into_iter()
                    .map(|(w, st, _)| json!({ "reviewer": w, "state": st }))
                    .collect();
            }
            let body = s(&pr, "body");
            if !body.trim().is_empty() {
                let (b, _) = clip(body.trim(), 6000, "description");
                out.push(&format!("\nDescription:\n{b}\n"));
            }
            Ok(done(
                c,
                out,
                json!({
                    "number": number, "title": s(&pr, "title"), "state": state, "draft": draft,
                    "author": login(&pr["user"]), "mergeable": pr.get("mergeable"),
                    "mergeable_state": pr.get("mergeable_state"),
                    "head_sha": s(&pr["head"], "sha"), "base": s(&pr["base"], "ref"),
                    "labels": l, "requested_reviewers": requested, "reviews": decisions,
                }),
                json!({ "number": number }),
            ))
        }
        .await,
    )
}

pub async fn pr_files<H: Http>(c: &Client<'_, H>, arguments: &str) -> Result<Reply, CallError> {
    let a = Args::parse(
        "gh_pr_files",
        arguments,
        &[
            "repo",
            "number",
            "max_count",
            "page",
            "patches",
            "max_patch_bytes",
        ],
    )?;
    let (owner, name) = repo(&a.req_str("repo")?)?;
    let number = pr_number(&a)?;
    let max = a.u64_or("max_count", 50, 1, 100)?;
    let page = a.u64_or("page", 1, 1, 30)?;
    let patches = a.bool_or("patches", true)?;
    let max_patch = a.u64_or("max_patch_bytes", 4096, 256, 16384)? as usize;
    finish(
        async {
            let files = c
                .get_json(
                    &format!("/repos/{owner}/{name}/pulls/{number}/files?per_page={max}&page={page}"),
                    &format!("pull request #{number} in {owner}/{name}"),
                )
                .await?;
            let files = files.as_array().cloned().unwrap_or_default();
            let mut out = Budget::default();
            out.push(&format!(
                "{} file{} changed in {owner}/{name}#{number} (page {page}):\n",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ));
            let mut rows = Vec::new();
            for f in &files {
                let n = |k: &str| f.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                let renamed = match s(f, "previous_filename") {
                    "" => String::new(),
                    p => format!(" (from {p})"),
                };
                if !out.push(&format!(
                    "{} {}{renamed} (+{} -{})\n",
                    s(f, "status"),
                    s(f, "filename"),
                    n("additions"),
                    n("deletions")
                )) {
                    break;
                }
                rows.push(json!({
                    "filename": s(f, "filename"), "status": s(f, "status"),
                    "additions": n("additions"), "deletions": n("deletions"),
                }));
            }
            if patches {
                let mut skipped = Vec::new();
                for f in &files {
                    let file = s(f, "filename");
                    if out.truncated() {
                        skipped.push(file.to_string());
                        continue;
                    }
                    let block = match f.get("patch").and_then(|p| p.as_str()) {
                        Some(p) => {
                            let (p, _) = clip(p, max_patch, &format!("patch for {file}"));
                            format!("\n--- {file}\n{p}\n")
                        }
                        None => format!(
                            "\n--- {file}\n(no patch: binary, too large for GitHub to show, or a rename only)\n"
                        ),
                    };
                    if !out.push_whole(&block) {
                        out.mark_truncated();
                        skipped.push(file.to_string());
                    }
                }
                if !skipped.is_empty() {
                    let names: Vec<String> = skipped.iter().take(30).cloned().collect();
                    out.set_note(format!(
                        "[output limit reached; patches not shown for {} files: {}. Lower \
                         max_patch_bytes or max_count, or use page.] {}",
                        skipped.len(),
                        names.join(", "),
                        c.rate_note()
                    ));
                }
            }
            if files.len() as u64 == max {
                out.push_whole(&format!(
                    "[there may be more files; call again with page={}]\n",
                    page + 1
                ));
            }
            Ok(done(
                c,
                out,
                json!({ "number": number, "files": rows }),
                json!({ "number": number, "count": files.len() }),
            ))
        }
        .await,
    )
}

pub async fn pr_checks<H: Http>(c: &Client<'_, H>, arguments: &str) -> Result<Reply, CallError> {
    let a = Args::parse("gh_pr_checks", arguments, &["repo", "number", "ref"])?;
    let (owner, name) = repo(&a.req_str("repo")?)?;
    let number = a.opt_u64("number", 1, 100_000_000)?;
    let git_ref = a.opt_nonempty("ref")?;
    if number.is_some() == git_ref.is_some() {
        return Err(CallError::invalid(
            "gh_pr_checks: give `number` (a pull request) or `ref` (a commit or branch), one of them",
        ));
    }
    finish(
        async {
            let (sha, label) = match (number, &git_ref) {
                (Some(n), _) => {
                    let pr = c
                        .get_json(
                            &format!("/repos/{owner}/{name}/pulls/{n}"),
                            &format!("pull request #{n} in {owner}/{name}"),
                        )
                        .await?;
                    (s(&pr["head"], "sha").to_string(), format!("#{n}"))
                }
                (None, Some(r)) => (r.clone(), r.clone()),
                _ => unreachable!(),
            };
            let what = format!("{label} in {owner}/{name}");
            let runs = c
                .get_json(
                    &format!("/repos/{owner}/{name}/commits/{}/check-runs?per_page=100", enc(&sha)),
                    &what,
                )
                .await?;
            let status = c
                .get_json(
                    &format!("/repos/{owner}/{name}/commits/{}/status?per_page=100", enc(&sha)),
                    &what,
                )
                .await?;
            let mut out = Budget::default();
            let (mut pass, mut fail, mut pending, mut other) = (0, 0, 0, 0);
            let mut lines = Vec::new();
            let mut rows = Vec::new();
            for r in runs.get("check_runs").and_then(|x| x.as_array()).into_iter().flatten() {
                let st = s(r, "status");
                let concl = r.get("conclusion").and_then(|x| x.as_str()).unwrap_or("");
                let verdict = if st != "completed" {
                    pending += 1;
                    st.to_string()
                } else {
                    match concl {
                        "success" | "neutral" | "skipped" => pass += 1,
                        "failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure" => fail += 1,
                        _ => other += 1,
                    }
                    concl.to_string()
                };
                lines.push((verdict.clone(), format!(
                    "{verdict:<10} {} ({})\n",
                    s(r, "name"),
                    s(&r["app"], "name")
                )));
                rows.push(json!({ "name": s(r, "name"), "status": st, "conclusion": concl }));
            }
            for st in status.get("statuses").and_then(|x| x.as_array()).into_iter().flatten() {
                let state = s(st, "state");
                match state {
                    "success" => pass += 1,
                    "failure" | "error" => fail += 1,
                    "pending" => pending += 1,
                    _ => other += 1,
                }
                lines.push((state.to_string(), format!(
                    "{state:<10} {} (status){}\n",
                    s(st, "context"),
                    match s(st, "description") {
                        "" => String::new(),
                        d => format!(": {}", one_line(d, 100)),
                    }
                )));
                rows.push(json!({ "name": s(st, "context"), "status": "status", "conclusion": state }));
            }
            let total = runs.get("total_count").and_then(|x| x.as_u64()).unwrap_or(0);
            out.push(&format!(
                "Checks for {what} at {}: {pass} passing, {fail} failing, {pending} pending{}\n",
                sha.get(..8).unwrap_or(&sha),
                if other > 0 { format!(", {other} other") } else { String::new() }
            ));
            if lines.is_empty() {
                out.push("No check runs or statuses reported for this commit.\n");
            }
            // Failures first, then pending, then the rest.
            let rank = |v: &str| match v {
                "failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure" | "error" => 0,
                "success" | "neutral" | "skipped" => 2,
                _ => 1,
            };
            lines.sort_by_key(|(v, _)| rank(v));
            for (_, l) in &lines {
                if !out.push(l) {
                    break;
                }
            }
            if total > 100 {
                out.push(&format!("[{total} check runs in all; the first 100 are shown]\n"));
            }
            Ok(done(
                c,
                out,
                json!({ "sha": sha, "passing": pass, "failing": fail, "pending": pending, "checks": rows }),
                json!({ "sha": sha, "passing": pass, "failing": fail, "pending": pending }),
            ))
        }
        .await,
    )
}

pub async fn pr_comments<H: Http>(c: &Client<'_, H>, arguments: &str) -> Result<Reply, CallError> {
    let a = Args::parse(
        "gh_pr_comments",
        arguments,
        &["repo", "number", "kind", "max_count", "max_body_bytes"],
    )?;
    let (owner, name) = repo(&a.req_str("repo")?)?;
    let number = pr_number(&a)?;
    let kind = a
        .opt_choice("kind", &["all", "conversation", "review"])?
        .unwrap_or_else(|| "all".into());
    let max = a.u64_or("max_count", 30, 1, 100)? as usize;
    let max_body = a.u64_or("max_body_bytes", 1000, 100, 8192)? as usize;
    finish(
        async {
            let what = format!("pull request #{number} in {owner}/{name}");
            // (created_at, line)
            let mut all: Vec<(String, String, Value)> = Vec::new();
            let mut maybe_more = false;
            if kind != "review" {
                let v = c
                    .get_json(&format!("/repos/{owner}/{name}/issues/{number}/comments?per_page=100"), &what)
                    .await?;
                let list = v.as_array().cloned().unwrap_or_default();
                maybe_more |= list.len() == 100;
                for cm in list {
                    let (body, _) = clip(s(&cm, "body").trim(), max_body, "comment");
                    let line = format!(
                        "[{}] {} commented:\n{}\n",
                        s(&cm, "created_at"),
                        login(&cm["user"]),
                        body.trim_end()
                    );
                    all.push((s(&cm, "created_at").to_string(), line,
                        json!({ "kind": "conversation", "author": login(&cm["user"]), "created_at": s(&cm, "created_at") })));
                }
            }
            if kind != "conversation" {
                let v = c
                    .get_json(&format!("/repos/{owner}/{name}/pulls/{number}/comments?per_page=100"), &what)
                    .await?;
                let list = v.as_array().cloned().unwrap_or_default();
                maybe_more |= list.len() == 100;
                for cm in list {
                    let (body, _) = clip(s(&cm, "body").trim(), max_body, "comment");
                    let line_no = cm.get("line").or_else(|| cm.get("original_line")).and_then(|l| l.as_u64());
                    let at = match line_no {
                        Some(l) => format!("{}:{l}", s(&cm, "path")),
                        None => s(&cm, "path").to_string(),
                    };
                    let reply = if cm.get("in_reply_to_id").is_some_and(|v| !v.is_null()) { " (reply)" } else { "" };
                    let line = format!(
                        "[{}] {} on {at}{reply}:\n{}\n",
                        s(&cm, "created_at"),
                        login(&cm["user"]),
                        body.trim_end()
                    );
                    all.push((s(&cm, "created_at").to_string(), line,
                        json!({ "kind": "review", "author": login(&cm["user"]), "created_at": s(&cm, "created_at"), "path": s(&cm, "path"), "line": line_no })));
                }
            }
            all.sort_by(|a, b| a.0.cmp(&b.0));
            let total = all.len();
            let dropped = total.saturating_sub(max);
            let kept = &all[dropped..];
            let mut out = Budget::default();
            out.push(&format!(
                "{total} {} comment{} on {owner}/{name}#{number}{}, oldest first:\n",
                if kind == "all" { "conversation and review".to_string() } else { kind.clone() },
                if total == 1 { "" } else { "s" },
                if dropped > 0 { format!(" (the {dropped} oldest left out; raise max_count)") } else { String::new() }
            ));
            if maybe_more {
                out.push("[GitHub returned 100 of one kind, so older comments may exist beyond this page]\n");
            }
            // Keep the newest that fit: render from the end, then reverse.
            let mut blocks = Vec::new();
            let mut left = out.remaining().saturating_sub(400);
            let mut cut = 0;
            for (_, line, _) in kept.iter().rev() {
                let b = format!("\n{line}");
                if b.len() > left {
                    cut += 1;
                    continue;
                }
                left -= b.len();
                blocks.push(b);
            }
            blocks.reverse();
            if cut > 0 {
                out.push(&format!("[{cut} older comments left out to fit the size limit]\n"));
            }
            for b in &blocks {
                out.push(b);
            }
            let rows: Vec<Value> = kept.iter().rev().take(60).rev().map(|(_, _, j)| j.clone()).collect();
            Ok(done(
                c,
                out,
                json!({ "number": number, "total": total, "comments": rows }),
                json!({ "number": number, "total": total }),
            ))
        }
        .await,
    )
}

pub async fn search_prs<H: Http>(c: &Client<'_, H>, arguments: &str) -> Result<Reply, CallError> {
    let a = Args::parse(
        "gh_search_prs",
        arguments,
        &["query", "repo", "state", "sort", "max_count"],
    )?;
    let query = a.req_str("query")?;
    if query.len() > 256 {
        return Err(CallError::invalid(
            "gh_search_prs: `query` is at most 256 characters",
        ));
    }
    let repo_q = match a.opt_nonempty("repo")? {
        Some(r) => {
            let (o, n) = repo(&r)?;
            format!(" repo:{o}/{n}")
        }
        None => String::new(),
    };
    let state = a.opt_choice("state", &["open", "closed", "all"])?;
    let sort = a
        .opt_choice("sort", &["best-match", "created", "updated", "comments"])?
        .unwrap_or_else(|| "best-match".into());
    let max = a.u64_or("max_count", 20, 1, 50)?;
    finish(
        async {
            let mut q = format!("is:pr {query}{repo_q}");
            match state.as_deref() {
                Some("open") => q.push_str(" is:open"),
                Some("closed") => q.push_str(" is:closed"),
                _ => {}
            }
            let mut path = format!("/search/issues?q={}&per_page={max}", enc(&q));
            if sort != "best-match" {
                path.push_str(&format!("&sort={sort}&order=desc"));
            }
            let v = c.get_json(&path, "the search").await.map_err(|e| match e {
                Fail::Failed(m) if m.contains("could not process") => Fail::Failed(format!(
                    "{m}. Check the query syntax: https://docs.github.com/search-github/searching-on-github/searching-issues-and-pull-requests"
                )),
                other => other,
            })?;
            let total = v.get("total_count").and_then(|t| t.as_u64()).unwrap_or(0);
            let items = v.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
            let mut out = Budget::default();
            out.push(&format!(
                "{total} pull request{} match `{q}`; showing {}:\n",
                if total == 1 { "" } else { "s" },
                items.len()
            ));
            if v.get("incomplete_results").and_then(|b| b.as_bool()) == Some(true) {
                out.push("[GitHub says these results are incomplete (the search timed out)]\n");
            }
            let mut rows = Vec::new();
            for it in &items {
                // https://api.github.com/repos/<owner>/<name>
                let repo_name = s(it, "repository_url").rsplitn(3, '/').take(2).collect::<Vec<_>>();
                let repo_name = if repo_name.len() == 2 { format!("{}/{}", repo_name[1], repo_name[0]) } else { String::new() };
                let n = it.get("number").and_then(|x| x.as_u64()).unwrap_or(0);
                let draft = if it.get("draft").and_then(|d| d.as_bool()) == Some(true) { ", draft" } else { "" };
                let l = labels(it);
                let line = format!(
                    "{repo_name}#{n} {} (by {}, {}{draft}, {} comments, updated {}){}\n",
                    one_line(s(it, "title"), 120),
                    login(&it["user"]),
                    s(it, "state"),
                    it.get("comments").and_then(|x| x.as_u64()).unwrap_or(0),
                    day(s(it, "updated_at")),
                    if l.is_empty() { String::new() } else { format!(" [labels: {}]", l.join(", ")) }
                );
                if !out.push(&line) {
                    break;
                }
                rows.push(json!({ "repo": repo_name, "number": n, "title": s(it, "title"), "state": s(it, "state") }));
            }
            Ok(done(
                c,
                out,
                json!({ "query": q, "total": total, "items": rows }),
                json!({ "total": total }),
            ))
        }
        .await,
    )
}
