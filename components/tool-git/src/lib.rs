//! git: read a repository under /repo, on gitoxide.
//!
//! git's famous escapes come from a repository telling git to run a program:
//! a hook, `core.fsmonitor`, `core.pager`, an `!alias`, a smudge or textconv
//! filter, a credential helper. gitoxide runs none of those on its own, and
//! this component can start no process even if it tried, so a hostile
//! repository's config does nothing. Only commands that read are offered;
//! anything that writes or fetches is refused. The boundary is the manifest
//! (one folder, mounted read only) and the read-only command set, not trust
//! in the repository.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/git-tool",
    generate_all,
});

use exports::workshop::toolbox::git::{Error, Guest, RunArgs};
use gix::bstr::{BString, ByteSlice};

/// The repository this tool reads. The manifest mounts the attendee's folder
/// here.
const REPO: &str = "/repo";

/// Open the repository, turning a missing or unreadable one into not-found.
fn open() -> Result<gix::Repository, Error> {
    gix::open(REPO).map_err(|e| Error::NotFound(format!("{REPO}: {e}")))
}

/// Shorthand for an invalid-argument error.
fn bad(msg: impl Into<String>) -> Error {
    Error::InvalidArgument(msg.into())
}

/// Anything that isn't a clean read gets reported, not run.
fn other(e: impl std::fmt::Display) -> Error {
    Error::Other(Some(e.to_string()))
}

struct Git;

impl Guest for Git {
    async fn run(args: RunArgs) -> Result<String, Error> {
        let words = split(&args.arguments)?;
        let mut rest: Vec<&str> = words.iter().map(String::as_str).collect();
        let cmd = match rest.first() {
            Some(c) => *c,
            None => return Err(bad("no command (try log, show, diff, blame, status, ls-files, cat-file)")),
        };
        rest.remove(0);
        let repo = open()?;
        match cmd {
            "log" => log(&repo, &rest),
            "show" => show(&repo, &rest),
            "diff" => diff(&repo, &rest),
            "blame" => blame(&repo, &rest),
            "status" => status(&repo),
            "ls-files" => ls_files(&repo),
            "ls-tree" => ls_tree(&repo, &rest),
            "cat-file" => cat_file(&repo, &rest),
            // Everything that writes or talks to another machine. Named so the
            // error is specific, rather than a flat "not available".
            "commit" | "add" | "rm" | "mv" | "push" | "pull" | "fetch" | "clone" | "merge"
            | "rebase" | "reset" | "checkout" | "switch" | "restore" | "tag" | "branch"
            | "config" | "remote" | "stash" | "apply" | "am" | "cherry-pick" | "revert"
            | "gc" | "clean" | "init" | "submodule" | "worktree" => Err(bad(format!(
                "'{cmd}' changes the repository or reaches the network; this git reads only"
            ))),
            other => Err(bad(format!(
                "'{other}' is not a command this git offers (read-only subset)"
            ))),
        }
    }
}

/// `log [-n N] [--oneline] [--format=FMT] [rev]`, newest first by commit time,
/// the way `git log` orders it.
fn log(repo: &gix::Repository, args: &[&str]) -> Result<String, Error> {
    let mut n = usize::MAX;
    let mut fmt = String::from("commit %H\nAuthor: %an <%ae>\nDate:   %ad\n\n    %s\n");
    let mut rev = String::from("HEAD");
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if a == "-n" || a == "--max-count" {
            n = args.get(i + 1).ok_or_else(|| bad("-n needs a number"))?.parse().map_err(|_| bad("-n needs a number"))?;
            i += 2;
            continue;
        } else if let Some(v) = a.strip_prefix("-n") {
            n = v.parse().map_err(|_| bad("-n needs a number"))?;
        } else if a == "--oneline" {
            fmt = "%h %s".into();
        } else if let Some(v) = a.strip_prefix("--format=").or_else(|| a.strip_prefix("--pretty=format:")) {
            fmt = v.into();
        } else if a.starts_with('-') {
            return Err(bad(format!("log: unknown option {a}")));
        } else {
            rev = a.to_string();
        }
        i += 1;
    }
    let start = repo.rev_parse_single(rev.as_str()).map_err(|e| Error::NotFound(format!("{rev}: {e}")))?;
    // sorting::ByCommitTime matches `git log`'s default order; the plain walk
    // is topological and can read oddly on a slide.
    let walk = repo
        .rev_walk([start.detach()])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(Default::default()))
        .all()
        .map_err(other)?;
    let mut out = String::new();
    for (i, info) in walk.enumerate() {
        if i >= n {
            break;
        }
        let c = info.map_err(other)?.object().map_err(other)?;
        if i > 0 && fmt.contains('\n') {
            out.push('\n');
        }
        out.push_str(fmt_commit(&c, &fmt)?.trim_end_matches('\n'));
        out.push('\n');
    }
    Ok(out)
}

/// `show [rev]`: the commit and the names it changed.
fn show(repo: &gix::Repository, args: &[&str]) -> Result<String, Error> {
    let rev = positional(args, "show")?.unwrap_or("HEAD");
    let c = repo
        .rev_parse_single(rev)
        .map_err(|e| Error::NotFound(format!("{rev}: {e}")))?
        .object()
        .map_err(other)?
        .peel_to_commit()
        .map_err(other)?;
    let mut out = fmt_commit(&c, "commit %H\nAuthor: %an <%ae>\nDate:   %ad\n\n    %s\n")?;
    let new = c.tree().map_err(other)?;
    let old = match c.parent_ids().next() {
        Some(p) => p.object().map_err(other)?.peel_to_commit().map_err(other)?.tree().map_err(other)?,
        None => repo.empty_tree(),
    };
    out.push_str(&changed_names(repo, &old, &new)?);
    Ok(out)
}

/// `diff <revA> <revB>`: the names that differ between two revisions.
fn diff(repo: &gix::Repository, args: &[&str]) -> Result<String, Error> {
    let revs: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    if revs.len() != 2 {
        return Err(bad("diff needs two revisions, e.g. diff HEAD~1 HEAD"));
    }
    let a = commit_tree(repo, revs[0])?;
    let b = commit_tree(repo, revs[1])?;
    changed_names(repo, &a, &b)
}

/// The changed names between two trees, as `M\tpath` lines. The patch text
/// isn't produced: it needs the worktree-filter path where textconv lives,
/// and the point here is that names read fine without running anything.
fn changed_names(repo: &gix::Repository, old: &gix::Tree<'_>, new: &gix::Tree<'_>) -> Result<String, Error> {
    use gix::object::tree::diff::ChangeDetached;
    let changes = repo.diff_tree_to_tree(Some(old), Some(new), None).map_err(other)?;
    let mut out = String::new();
    for ch in &changes {
        if ch.entry_mode().is_tree() {
            continue;
        }
        let (tag, loc) = match ch {
            ChangeDetached::Addition { location, .. } => ("A", location.to_string()),
            ChangeDetached::Deletion { location, .. } => ("D", location.to_string()),
            ChangeDetached::Modification { location, .. } => ("M", location.to_string()),
            ChangeDetached::Rewrite { source_location, location, .. } => {
                ("R", format!("{source_location} -> {location}"))
            }
        };
        out.push_str(&format!("{tag}\t{loc}\n"));
    }
    Ok(out)
}

/// `blame <file> [rev]`: who last touched each line.
fn blame(repo: &gix::Repository, args: &[&str]) -> Result<String, Error> {
    let file = positional(args, "blame")?.ok_or_else(|| bad("blame needs a file"))?;
    let rev = args.iter().copied().filter(|a| !a.starts_with('-')).nth(1).unwrap_or("HEAD");
    let suspect = repo
        .rev_parse_single(rev)
        .map_err(|e| Error::NotFound(format!("{rev}: {e}")))?
        .object()
        .map_err(other)?
        .peel_to_commit()
        .map_err(other)?
        .id;
    let out = repo
        .blame_file(file.into(), suspect, Default::default())
        .map_err(|e| Error::NotFound(format!("{file}: {e}")))?;
    let mut s = String::new();
    for (entry, lines) in out.entries_with_lines() {
        let c = repo.find_commit(entry.commit_id).map_err(other)?;
        let a = c.author().map_err(other)?;
        let short = entry.commit_id.to_hex_with_len(8).to_string();
        for (i, line) in lines.iter().enumerate() {
            let num = entry.start_in_blamed_file as usize + i + 1;
            s.push_str(&format!("{short} ({:<14} {num:>3}) {}", a.name.to_str_lossy(), line));
            if !line.ends_with(b"\n") {
                s.push('\n');
            }
        }
    }
    Ok(s)
}

/// `status`: a short summary. With no worktree changes to compute cheaply,
/// this reports the branch and the tip, which is what a slide needs.
fn status(repo: &gix::Repository) -> Result<String, Error> {
    let head = repo.head().map_err(other)?;
    let branch = head
        .referent_name()
        .map(|n| n.shorten().to_string())
        .unwrap_or_else(|| "(detached)".into());
    let tip = repo
        .rev_parse_single("HEAD")
        .map(|id| id.shorten_or_id().to_string())
        .unwrap_or_else(|_| "(no commits)".into());
    Ok(format!("On branch {branch}\nHEAD is {tip}\n"))
}

/// `ls-files`: the paths tracked in the index.
fn ls_files(repo: &gix::Repository) -> Result<String, Error> {
    let index = repo.index_or_load_from_head().map_err(other)?;
    let mut out = String::new();
    for e in index.entries() {
        out.push_str(&format!("{}\n", e.path(&index)));
    }
    Ok(out)
}

/// `ls-tree [-r] [rev]`: the entries of a tree.
fn ls_tree(repo: &gix::Repository, args: &[&str]) -> Result<String, Error> {
    let recursive = args.contains(&"-r");
    let rev = args.iter().copied().filter(|a| !a.starts_with('-')).next().unwrap_or("HEAD");
    let tree = commit_tree(repo, rev)?;
    let mut out = String::new();
    if recursive {
        let mut rec = gix::traverse::tree::Recorder::default();
        tree.traverse().breadthfirst(&mut rec).map_err(other)?;
        for e in rec.records.iter().filter(|e| !e.mode.is_tree()) {
            out.push_str(&format!("{:06o} {} {}\t{}\n", e.mode.value(), kind(e.mode), e.oid, e.filepath));
        }
    } else {
        for e in tree.iter() {
            let e = e.map_err(other)?;
            out.push_str(&format!("{:06o} {} {}\t{}\n", e.mode().value(), kind(e.mode()), e.oid(), e.filename()));
        }
    }
    Ok(out)
}

/// `cat-file -p <rev>`: the contents of a blob or the text of a commit/tree.
fn cat_file(repo: &gix::Repository, args: &[&str]) -> Result<String, Error> {
    let rev = args.iter().copied().filter(|a| !a.starts_with('-')).next().ok_or_else(|| bad("cat-file needs an object, e.g. cat-file -p HEAD:README.md"))?;
    let obj = repo
        .rev_parse_single(rev)
        .map_err(|e| Error::NotFound(format!("{rev}: {e}")))?
        .object()
        .map_err(other)?;
    Ok(String::from_utf8_lossy(&obj.data).into_owned())
}

fn commit_tree<'r>(repo: &'r gix::Repository, rev: &str) -> Result<gix::Tree<'r>, Error> {
    repo.rev_parse_single(rev)
        .map_err(|e| Error::NotFound(format!("{rev}: {e}")))?
        .object()
        .map_err(other)?
        .peel_to_commit()
        .map_err(other)?
        .tree()
        .map_err(other)
}

fn fmt_commit(c: &gix::Commit<'_>, fmt: &str) -> Result<String, Error> {
    let author = c.author().map_err(other)?;
    let time = author.time().map_err(other)?;
    let id = c.id();
    let msg = c.message().map_err(other)?;
    let out = fmt
        .replace("\\n", "\n")
        .replace("%H", &id.to_string())
        .replace("%h", &id.shorten_or_id().to_string())
        .replace("%an", &author.name.to_str_lossy())
        .replace("%ae", &author.email.to_str_lossy())
        .replace("%ad", &time.format_or_unix(gix::date::time::format::DEFAULT))
        .replace("%s", &msg.summary().to_str_lossy());
    Ok(out)
}

fn kind(m: gix::object::tree::EntryMode) -> &'static str {
    if m.is_tree() {
        "tree"
    } else if m.is_commit() {
        "commit"
    } else {
        "blob"
    }
}

/// The first argument that isn't an option.
fn positional<'a>(args: &'a [&'a str], _cmd: &str) -> Result<Option<&'a str>, Error> {
    Ok(args.iter().copied().find(|a| !a.starts_with('-')))
}

/// Split a command line, honoring single and double quotes, so a path with a
/// space works. A dangling quote is an error, not a silent join.
fn split(line: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_word = false;
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' | '"' => {
                in_word = true;
                let quote = c;
                loop {
                    match chars.next() {
                        Some(q) if q == quote => break,
                        Some(c) => cur.push(c),
                        None => return Err(bad("unbalanced quote")),
                    }
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    let _ = BString::from("");
    Ok(out)
}

export!(Git);
