//! Read-only git subset on gitoxide, built for wasm32-wasip2.
//!
//! usage: git-spike [-C <path>] <command> [args]
//!   log [-n N] [--format=FMT] [rev]      FMT tokens: %H %h %an %ae %ad %s
//!   show [-p] [--textconv] <rev>
//!   diff [-p] [--textconv] <rev> <rev>
//!   blame <file> [rev]
//!   ls-files
//!   ls-tree [-r] [rev]
//!   grep <needle> [rev]
//!   config-probe                         print dangerous keys gix parsed

use gix::bstr::{BStr, BString, ByteSlice};
use gix::diff::blob::{
    pipeline::Mode,
    platform::prepare_diff::Operation,
    unified_diff::{ConsumeBinaryHunk, ContextSize},
    Diff, ResourceKind, UnifiedDiff,
};
use gix::object::tree::diff::ChangeDetached;
use std::process::ExitCode;

type R<T> = Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fatal: {e}"); if std::env::var_os("GIT_SPIKE_DEBUG").is_some() { eprintln!("{e:?}"); }
            ExitCode::from(128)
        }
    }
}

fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(msg.into().into())
}

fn run() -> R<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut path = String::from(".");
    if args.first().map(|s| s.as_str()) == Some("-C") {
        if args.len() < 2 {
            return err("-C needs a path");
        }
        path = args[1].clone();
        args.drain(..2);
    }
    if args.is_empty() {
        return err("no command (log, show, diff, blame, ls-files, ls-tree, grep, config-probe)");
    }
    let cmd = args.remove(0);
    let repo = gix::open(&path).map_err(|e| format!("cannot open repository at '{path}': {e}"))?;
    let flag = |args: &mut Vec<String>, f: &str| -> bool {
        if let Some(i) = args.iter().position(|a| a == f) {
            args.remove(i);
            true
        } else {
            false
        }
    };
    match cmd.as_str() {
        "log" => {
            let mut n = usize::MAX;
            let mut fmt = String::from("commit %H\nAuthor: %an <%ae>\nDate:   %ad\n\n    %s\n");
            let mut rev = String::from("HEAD");
            let mut i = 0;
            while i < args.len() {
                let a = &args[i];
                if a == "-n" {
                    n = args.get(i + 1).ok_or("-n needs a number")?.parse()?;
                    i += 2;
                    continue;
                } else if let Some(v) = a.strip_prefix("-n") {
                    n = v.parse()?;
                } else if a == "--oneline" {
                    fmt = "%h %s".into();
                } else if let Some(v) = a.strip_prefix("--format=").or(a.strip_prefix("--pretty=format:")) {
                    fmt = v.into();
                } else {
                    rev = a.clone();
                }
                i += 1;
            }
            log(&repo, &rev, n, &fmt)
        }
        "show" => {
            let patch = flag(&mut args, "-p");
            let textconv = flag(&mut args, "--textconv");
            let rev = args.first().cloned().unwrap_or_else(|| "HEAD".into());
            show(&repo, &rev, patch, textconv)
        }
        "diff" => {
            let patch = flag(&mut args, "-p");
            let textconv = flag(&mut args, "--textconv");
            if args.len() != 2 {
                return err("diff needs two revisions");
            }
            let a = commit_tree(&repo, &args[0])?;
            let b = commit_tree(&repo, &args[1])?;
            print_changes(&repo, &a, &b, patch, textconv)
        }
        "blame" => {
            let file = args.first().ok_or("blame needs a file")?;
            let rev = args.get(1).cloned().unwrap_or_else(|| "HEAD".into());
            blame(&repo, file, &rev)
        }
        "ls-files" => {
            let index = repo.index_or_load_from_head()?;
            for e in index.entries() {
                println!("{}", e.path(&index));
            }
            Ok(())
        }
        "ls-tree" => {
            let recursive = flag(&mut args, "-r");
            let rev = args.first().cloned().unwrap_or_else(|| "HEAD".into());
            let tree = commit_tree(&repo, &rev)?;
            if recursive {
                let mut rec = gix::traverse::tree::Recorder::default();
                tree.traverse().breadthfirst(&mut rec)?;
                for e in rec.records.iter().filter(|e| !e.mode.is_tree()) {
                    println!("{:06o} {} {}\t{}", e.mode.value(), kind(e.mode), e.oid, e.filepath);
                }
            } else {
                for e in tree.iter() {
                    let e = e?;
                    println!("{:06o} {} {}\t{}", e.mode().value(), kind(e.mode()), e.oid(), e.filename());
                }
            }
            Ok(())
        }
        "grep" => {
            let needle = args.first().ok_or("grep needs a pattern")?.clone();
            let rev = args.get(1).cloned().unwrap_or_else(|| "HEAD".into());
            grep(&repo, &needle, &rev)
        }
        "config-probe" => config_probe(&repo),
        "paths" => { println!("git_dir={:?} workdir={:?} common={:?}", repo.git_dir(), repo.workdir(), repo.common_dir()); println!("{:?}", std::fs::read_to_string(repo.git_dir().join("HEAD"))); Ok(()) }
        other => err(format!("'{other}' is not a supported command (read-only subset)")),
    }
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

fn commit_tree<'r>(repo: &'r gix::Repository, rev: &str) -> R<gix::Tree<'r>> {
    let id = repo.rev_parse_single(rev)?;
    Ok(id.object()?.peel_to_commit()?.tree()?)
}

fn fmt_commit(c: &gix::Commit<'_>, fmt: &str) -> R<String> {
    let author = c.author()?;
    let time = author.time()?;
    let id = c.id();
    let msg = c.message()?;
    let mut out = fmt.replace("\\n", "\n");
    out = out
        .replace("%H", &id.to_string())
        .replace("%h", &id.shorten_or_id().to_string())
        .replace("%an", &author.name.to_str_lossy())
        .replace("%ae", &author.email.to_str_lossy())
        .replace("%ad", &time.format_or_unix(gix::date::time::format::DEFAULT))
        .replace("%s", &msg.summary().to_str_lossy());
    Ok(out)
}

fn log(repo: &gix::Repository, rev: &str, n: usize, fmt: &str) -> R<()> {
    let start = repo.rev_parse_single(rev)?;
    let walk = repo.rev_walk([start.detach()]).all()?;
    for (i, info) in walk.enumerate() {
        if i >= n {
            break;
        }
        let info = info?;
        let c = info.object()?;
        if i > 0 && fmt.contains('\n') {
            println!();
        }
        println!("{}", fmt_commit(&c, fmt)?.trim_end_matches('\n'));
    }
    Ok(())
}

fn show(repo: &gix::Repository, rev: &str, patch: bool, textconv: bool) -> R<()> {
    let c = repo.rev_parse_single(rev)?.object()?.peel_to_commit()?;
    println!(
        "{}",
        fmt_commit(&c, "commit %H\nAuthor: %an <%ae>\nDate:   %ad\n\n    %s\n")?
    );
    let new = c.tree()?;
    let old = match c.parent_ids().next() {
        Some(p) => p.object()?.peel_to_commit()?.tree()?,
        None => repo.empty_tree(),
    };
    print_changes(repo, &old, &new, patch, textconv)
}

fn print_changes(
    repo: &gix::Repository,
    old: &gix::Tree<'_>,
    new: &gix::Tree<'_>,
    patch: bool,
    textconv: bool,
) -> R<()> {
    let changes = repo.diff_tree_to_tree(Some(old), Some(new), None)?;
    for ch in &changes {
        let (tag, loc) = match ch {
            ChangeDetached::Addition { location, .. } => ("A", location.to_string()),
            ChangeDetached::Deletion { location, .. } => ("D", location.to_string()),
            ChangeDetached::Modification { location, .. } => ("M", location.to_string()),
            ChangeDetached::Rewrite { source_location, location, .. } => {
                ("R", format!("{source_location} -> {location}"))
            }
        };
        if !ch.entry_mode().is_tree() {
            println!("{tag}\t{loc}");
        }
    }
    if !patch {
        return Ok(());
    }
    // textconv=true asks gix to prepare blobs "as the worktree would see them", which is
    // where smudge filters and diff.<driver>.textconv come into play (both spawn processes).
    let mode = if textconv { Mode::ToWorktreeAndBinaryToText } else { Mode::ToGit };
    let mut cache = repo.diff_resource_cache(mode, Default::default())?;
    let null = gix::ObjectId::null(repo.object_hash());
    for ch in &changes {
        if ch.entry_mode().is_tree() {
            continue;
        }
        let (old_id, old_kind, new_id, new_kind, loc): (_, _, _, _, &BStr) = match ch {
            ChangeDetached::Addition { location, entry_mode, id, .. } => {
                (null, entry_mode.kind(), *id, entry_mode.kind(), location.as_ref())
            }
            ChangeDetached::Deletion { location, entry_mode, id, .. } => {
                (*id, entry_mode.kind(), null, entry_mode.kind(), location.as_ref())
            }
            ChangeDetached::Modification { location, previous_entry_mode, previous_id, entry_mode, id } => {
                (*previous_id, previous_entry_mode.kind(), *id, entry_mode.kind(), location.as_ref())
            }
            ChangeDetached::Rewrite { source_id, source_entry_mode, entry_mode, id, location, .. } => {
                (*source_id, source_entry_mode.kind(), *id, entry_mode.kind(), location.as_ref())
            }
        };
        println!("diff --git a/{loc} b/{loc}");
        if let Err(e) = cache.set_resource(old_id, old_kind, loc, ResourceKind::OldOrSource, &repo.objects) {
            println!("  (could not load old side: {e})"); if std::env::var_os("GIT_SPIKE_DEBUG").is_some() { println!("{e:?}"); }
            continue;
        }
        if let Err(e) = cache.set_resource(new_id, new_kind, loc, ResourceKind::NewOrDestination, &repo.objects) {
            println!("  (could not load new side: {e})");
            continue;
        }
        let out = match cache.prepare_diff() {
            Ok(o) => o,
            Err(e) => {
                println!("  (could not prepare diff: {e:?})");
                continue;
            }
        };
        match out.operation {
            Operation::InternalDiff { algorithm } => {
                let input = out.interned_input();
                let diff = Diff::compute(algorithm, &input);
                let text: BString = UnifiedDiff::new(
                    &diff,
                    &input,
                    ConsumeBinaryHunk::new(BString::default(), "\n"),
                    ContextSize::symmetrical(3),
                )
                .consume()?;
                print!("--- a/{loc}\n+++ b/{loc}\n{text}");
            }
            Operation::ExternalCommand { command } => {
                println!("  (external diff command configured, NOT run by this tool: {command})");
            }
            Operation::SourceOrDestinationIsBinary => println!("Binary files differ"),
        }
    }
    Ok(())
}

fn blame(repo: &gix::Repository, file: &str, rev: &str) -> R<()> {
    let suspect = repo.rev_parse_single(rev)?.object()?.peel_to_commit()?.id;
    let out = repo.blame_file(file.into(), suspect, Default::default())?;
    for (entry, lines) in out.entries_with_lines() {
        let c = repo.find_commit(entry.commit_id)?;
        let a = c.author()?;
        let short = entry.commit_id.to_hex_with_len(8).to_string();
        for (i, line) in lines.iter().enumerate() {
            let n = entry.start_in_blamed_file as usize + i + 1;
            print!("{short} ({:<14} {n:>3}) {}", a.name.to_str_lossy(), line);
            if !line.ends_with(b"\n") {
                println!();
            }
        }
    }
    Ok(())
}

fn grep(repo: &gix::Repository, needle: &str, rev: &str) -> R<()> {
    let tree = commit_tree(repo, rev)?;
    let mut rec = gix::traverse::tree::Recorder::default();
    tree.traverse().breadthfirst(&mut rec)?;
    let mut hits = 0;
    for e in rec.records.iter().filter(|e| e.mode.is_blob()) {
        let blob = repo.find_blob(e.oid)?;
        if blob.data.contains(&0) {
            continue;
        }
        for (i, line) in blob.data.lines().enumerate() {
            if line.find(needle.as_bytes()).is_some() {
                println!("{}:{}:{}", e.filepath, i + 1, line.to_str_lossy());
                hits += 1;
            }
        }
    }
    if hits == 0 {
        return err("no matches");
    }
    Ok(())
}

fn config_probe(repo: &gix::Repository) -> R<()> {
    let snap = repo.config_snapshot();
    let keys = [
        "core.fsmonitor",
        "core.pager",
        "core.sshCommand",
        "core.editor",
        "core.hooksPath",
        "core.askPass",
        "alias.lg",
        "alias.log",
        "filter.evil.smudge",
        "filter.evil.clean",
        "filter.evil.process",
        "diff.evil.textconv",
        "diff.evil.command",
        "diff.external",
        "credential.helper",
        "gpg.program",
        "remote.origin.url",
    ];
    for k in keys {
        match snap.string(k) {
            Some(v) => println!("{k} = {v}"),
            None => println!("{k} (not set)"),
        }
    }
    println!("git_dir trust: {:?}", repo.git_dir_trust());
    Ok(())
}
