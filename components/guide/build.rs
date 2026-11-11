//! Renders the workshop's READMEs to finished HTML pages, once, at build time.
//!
//! The pages come straight from the repo: README.md is the home page and each
//! steps/*/README.md is a page of its own, so the guide never holds a copy of
//! the text to keep in sync. Markdown is rendered here rather than in the
//! component, so the component carries only the pages (no Markdown parser)
//! and answers each request with a string it already has.
//!
//! The brand (the wasmCloud logo and the colour palette) is read from the agent's
//! chat page, components/agent/src/transcript.html, so the two pages match.
//!
//! What the rendering adds to plain Markdown:
//!   * every code block gets a label: Shell (```sh), Chat (```chat, a prompt
//!     for the chat page), YAML, or Output (```text, or no language right
//!     after a command). Only Shell and Chat get a Copy button: they are what
//!     the reader sends. Output is styled quieter, to be read, not run
//!   * what lies between `<!-- codespaces -->` and `<!-- /codespaces -->` is
//!     left out: each prompt's `./ask` form and the lines that point to it.
//!     The guide runs only on the reader's own machine, where the chat page
//!     works; on GitHub the comments don't show and Codespaces readers keep
//!     those lines. An unbalanced marker fails the build
//!   * URLs a browser can open become links (in text and in code), but not
//!     the ones that only work with a Host header or inside the cluster
//!   * links between READMEs, and inline code naming a step folder, go to
//!     that page; links to other repo files go to GitHub
//!   * "(allowed)" and "(refused: ...)" become small badges
//!
//! Each page is also written to $OUT_DIR/site/, to look at without a cluster.
//! It also writes the POSIX `cksum` of each README, served at /sources, so
//! `make verify` can tell when the prebuilt guide is older than the READMEs.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

const REPO: &str = "https://github.com/cosmonic-labs/sandbox-workshop-kcna26";
/// Sidebar link icons, 16x16. GITHUB_ICON is GitHub's mark (Octicons, MIT).
const GITHUB_ICON: &str = "<path fill=\"currentColor\" d=\"M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z\"/>";
const CHAT_ICON: &str = "<path fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.4\" stroke-linejoin=\"round\" d=\"M2.5 3.2h11v7.6H7.2L4.3 13.3v-2.5H2.5z\"/>";
const WEB_ICON: &str = "<g fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.3\"><circle cx=\"8\" cy=\"8\" r=\"6.3\"/><path d=\"M1.7 8h12.6M8 1.7c-2.2 2.4-2.2 10.2 0 12.6M8 1.7c2.2 2.4 2.2 10.2 0 12.6\"/></g>";
const CHAT: &str = "http://agent.localhost:28180";
/// Around what only Codespaces readers need, which the guide leaves out.
const CODESPACES_OPEN: &str = "<!-- codespaces -->";
const CODESPACES_CLOSE: &str = "<!-- /codespaces -->";
/// Folders with a README that the guide serves but that are not steps.
const NOT_STEPS: [&str; 1] = ["mock-sites"];

struct Page {
    /// The README's folder, relative to the repo root: "" or "steps/01-agent".
    dir: String,
    /// Where the guide serves it: "/" or "/steps/01-agent/".
    route: String,
    md: String,
    /// The README's first heading.
    title: String,
    /// The title without "Step N: ".
    short: String,
    /// 1 to 9 for steps/0N-*; none for the home page and NOT_STEPS.
    step: Option<u32>,
    /// The first paragraph of prose, for the step list on the home page.
    summary: String,
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..").canonicalize().unwrap();
    let rerun = |p: &Path| println!("cargo:rerun-if-changed={}", p.display());

    let mut dirs = vec![String::new()];
    let mut steps: Vec<String> = fs::read_dir(root.join("steps"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("README.md").is_file())
        .map(|e| format!("steps/{}", e.file_name().to_string_lossy()))
        .collect();
    steps.sort();
    dirs.extend(steps);
    rerun(&root.join("steps"));
    // Pages that are not steps, after the steps.
    for d in NOT_STEPS {
        if root.join(d).join("README.md").is_file() {
            dirs.push(d.to_string());
        }
    }

    let mut pages: Vec<Page> = dirs.iter().map(|d| load(&root, d)).collect();
    // A step's line in the home page's step list: its row in README.md's
    // Steps table when it has one, else its README's first paragraph.
    for (dir, said) in table_rows(&pages[0].md) {
        if let Some(p) = pages.iter_mut().find(|p| p.dir == dir) {
            p.summary = said;
        }
    }
    for p in &pages {
        rerun(&readme(&root, &p.dir));
    }

    let chat = root.join("components/agent/src/transcript.html");
    for f in ["src/layout.html", "src/guide.css", "src/guide.js"] {
        rerun(&manifest.join(f));
    }
    rerun(&chat);
    let chat = fs::read_to_string(&chat).unwrap();
    let logo = between(&chat, "<svg class=\"logo\"", "</svg>");
    let logo = format!("<svg class=\"logo\"{logo}</svg>");
    // The favicon: the logo's hexagon mark alone (its "m" path, in the logo's
    // mark colour), centred in a square, since the wordmark is unreadable at 16px.
    let mark = between(&logo, "<path class=\"m\" d=\"", "\"");
    let favicon = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-2.61 0 42.03 42.03\">\
         <path fill=\"#00bc8e\" d=\"{mark}\"/></svg>\n"
    );
    // The palette: from the comment that introduces it to the first rule after it.
    let palette = between(&chat, "  /* wasmCloud palette", "  * { box-sizing");
    let palette = format!("  /* wasmCloud palette{palette}");
    let css = fs::read_to_string(manifest.join("src/guide.css")).unwrap();
    let css = format!("{palette}\n{css}");
    let js = fs::read_to_string(manifest.join("src/guide.js")).unwrap();
    let layout = fs::read_to_string(manifest.join("src/layout.html")).unwrap();
    let layout = layout.replace("{{js}}", &js).replace("{{logo}}", &logo);

    let site = PathBuf::from(env::var("OUT_DIR").unwrap()).join("site");
    let mut out = String::from("// Generated by build.rs from the repo's READMEs. Do not edit.\n");
    out.push_str("pub static PAGES: &[(&str, &str)] = &[\n");
    for (i, p) in pages.iter().enumerate() {
        let body = render(&root, &pages, i);
        let html = layout
            .replace("{{title}}", &esc(&title_of(p)))
            .replace("{{nav}}", &nav(&pages, i))
            .replace("{{pager}}", &pager(&pages, i))
            .replace("{{content}}", &body);
        writeln!(out, "    ({:?}, {:?}),", p.route, html).unwrap();
        // The same page as a file, to look at without a cluster.
        let file = site.join(p.route.trim_start_matches('/')).join("index.html");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, &html).unwrap();
    }
    out.push_str("];\n");
    let missing = "<h1>Not in this guide</h1><p>There is no page here. \
        <a href=\"/\">Start from the overview</a>.</p>";
    let not_found = layout
        .replace("{{title}}", "Not found · Workshop guide")
        .replace("{{nav}}", &nav(&pages, usize::MAX))
        .replace("{{pager}}", "")
        .replace("{{content}}", missing);
    writeln!(out, "pub static NOT_FOUND: &str = {not_found:?};").unwrap();

    let mut sums = String::new();
    for p in &pages {
        let bytes = fs::read(readme(&root, &p.dir)).unwrap();
        let path = if p.dir.is_empty() { "README.md".into() } else { format!("{}/README.md", p.dir) };
        writeln!(sums, "{} {} {path}", cksum(&bytes), bytes.len()).unwrap();
    }
    writeln!(out, "pub static SOURCES: &str = {sums:?};").unwrap();
    // One stylesheet for every page, so each page doesn't carry its own copy.
    writeln!(out, "pub static CSS: &str = {css:?};").unwrap();
    fs::write(site.join("guide.css"), &css).unwrap();
    writeln!(out, "pub static FAVICON: &str = {favicon:?};").unwrap();
    fs::write(site.join("favicon.svg"), &favicon).unwrap();

    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("pages.rs"), out).unwrap();
}

fn readme(root: &Path, dir: &str) -> PathBuf {
    if dir.is_empty() { root.join("README.md") } else { root.join(dir).join("README.md") }
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> &'a str {
    let a = s.find(start).unwrap_or_else(|| panic!("transcript.html: no {start:?}")) + start.len();
    let b = a + s[a..].find(end).unwrap_or_else(|| panic!("transcript.html: no {end:?}"));
    &s[a..b]
}

fn load(root: &Path, dir: &str) -> Page {
    let md = fs::read_to_string(readme(root, dir)).unwrap();
    let name = dir.rsplit('/').next().unwrap_or("");
    let step = name.get(..2).and_then(|n| n.parse::<u32>().ok()).filter(|n| *n > 0);
    let route = if dir.is_empty() { "/".to_string() } else { format!("/{dir}/") };
    // The first heading and the first paragraph that isn't the Folder line.
    let (mut title, mut summary) = (String::new(), String::new());
    let (mut in_h1, mut in_p, mut para) = (false, false, String::new());
    for ev in Parser::new_ext(&md, options()) {
        match ev {
            Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }) if title.is_empty() => in_h1 = true,
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => in_h1 = false,
            Event::Start(Tag::Paragraph) => (in_p, para) = (true, String::new()),
            Event::End(TagEnd::Paragraph) => {
                in_p = false;
                if summary.is_empty() && !title.is_empty() && !para.starts_with("Folder:") {
                    summary = para.trim().to_string();
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if in_h1 {
                    title.push_str(&t);
                } else if in_p {
                    para.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak if in_p => para.push(' '),
            _ => {}
        }
    }
    let short = match title.split_once(": ") {
        Some((head, rest)) if head.starts_with("Step ") => rest.to_string(),
        _ => title.clone(),
    };
    Page { dir: dir.to_string(), route, md, title, short, step, summary }
}

/// Rows of README.md's tables whose second cell names a folder: (folder, last cell).
fn table_rows(md: &str) -> Vec<(String, String)> {
    let (mut rows, mut row, mut cell) = (Vec::new(), Vec::<String>::new(), None::<String>);
    for ev in Parser::new_ext(md, options()) {
        match ev {
            Event::Start(Tag::TableRow) => row.clear(),
            Event::Start(Tag::TableCell) => cell = Some(String::new()),
            Event::End(TagEnd::TableCell) => row.extend(cell.take()),
            Event::Text(t) | Event::Code(t) => {
                if let Some(c) = &mut cell {
                    c.push_str(&t);
                }
            }
            Event::End(TagEnd::TableRow) if row.len() >= 3 => {
                rows.push((row[1].trim().trim_end_matches('/').to_string(), row[row.len() - 1].trim().to_string()))
            }
            _ => {}
        }
    }
    rows
}

fn title_of(p: &Page) -> String {
    if p.route == "/" { "Workshop guide".into() } else { format!("{} · Workshop guide", p.title) }
}

fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            o.push(c);
        } else if (c == ' ' || c == '-') && !o.ends_with('-') {
            o.push('-');
        }
    }
    o.trim_matches('-').to_string()
}

/// The nav: the overview, each step in order (the current one marked), then
/// the pages that aren't steps.
fn nav(pages: &[Page], cur: usize) -> String {
    let mut o = String::from("<nav class=\"toc\" aria-label=\"Workshop steps\"><ol>");
    for (i, p) in pages.iter().enumerate() {
        let num = match (p.route.as_str(), p.step) {
            ("/", _) => "&#8962;".to_string(),
            (_, Some(n)) => n.to_string(),
            _ => continue,
        };
        let label = if p.route == "/" { "Overview".to_string() } else { esc(&p.short) };
        let current = if i == cur { " aria-current=\"page\"" } else { "" };
        write!(
            o,
            "<li><a href=\"{}\"{current}><span class=\"num\" aria-hidden=\"true\">{num}</span>\
             <span class=\"lbl\">{}{label}</span></a></li>",
            p.route,
            p.step.map(|n| format!("<span class=\"sr-only\">Step {n}: </span>")).unwrap_or_default(),
        )
        .unwrap();
    }
    // The pages that are not steps (NOT_STEPS) stay out of the list:
    // the README and step 2 link to them where they matter.
    let link = |href: &str, icon: &str, label: &str| {
        format!(
            "<li><a href=\"{href}\" target=\"_blank\" rel=\"noopener\">\
             <svg class=\"ic\" viewBox=\"0 0 16 16\" aria-hidden=\"true\">{icon}</svg>\
             <span>{label}</span></a></li>"
        )
    };
    o.push_str("</ol><ul class=\"also\">");
    o.push_str(&link(CHAT, CHAT_ICON, "Agent chat (after step 1)"));
    o.push_str(&link(REPO, GITHUB_ICON, "<code>sandbox-workshop-kcna26</code>"));
    o.push_str(&link("https://github.com/wasmCloud/wasmCloud", GITHUB_ICON, "<code>wasmCloud/wasmCloud</code>"));
    o.push_str(&link("https://wasmcloud.com", WEB_ICON, "wasmcloud.com"));
    o.push_str("</ul></nav>");
    o
}

/// Previous and next, in reading order: the overview, then the steps.
fn pager(pages: &[Page], cur: usize) -> String {
    let order: Vec<usize> =
        (0..pages.len()).filter(|&i| pages[i].route == "/" || pages[i].step.is_some()).collect();
    let Some(pos) = order.iter().position(|&i| i == cur) else {
        return "<nav class=\"pager\" aria-label=\"Previous and next\"><a class=\"prev\" href=\"/\" rel=\"prev\">\
             <span class=\"kicker\">Back to</span><span class=\"t\">Overview</span></a></nav>".to_string();
    };
    let link = |i: usize, rel: &str, kicker: &str| {
        let p = &pages[i];
        let t = if p.route == "/" { "Overview".to_string() } else { esc(&p.title) };
        format!("<a class=\"{rel}\" href=\"{}\" rel=\"{rel}\"><span class=\"kicker\">{kicker}</span><span class=\"t\">{t}</span></a>", p.route)
    };
    let mut o = String::from("<nav class=\"pager\" aria-label=\"Previous and next\">");
    if pos > 0 {
        o.push_str(&link(order[pos - 1], "prev", "Previous"));
    }
    if pos + 1 < order.len() {
        o.push_str(&link(order[pos + 1], "next", if pos == 0 { "Start" } else { "Next" }));
    } else {
        write!(o, "<a class=\"next\" href=\"{CHAT}\" target=\"_blank\" rel=\"noopener\"><span class=\"kicker\">Keep going</span><span class=\"t\">Open the agent chat</span></a>").unwrap();
    }
    o.push_str("</nav>");
    o
}

/// The home page's step list, with the line about the guide itself.
fn step_list(pages: &[Page]) -> String {
    let mut o = String::from("<nav class=\"steps\" aria-label=\"The steps\"><ol>");
    for p in pages.iter().filter(|p| p.step.is_some()) {
        write!(
            o,
            "<li><a href=\"{}\"><span class=\"num\" aria-hidden=\"true\">{}</span><span class=\"body\">\
             <span class=\"t\"><span class=\"sr-only\">Step {}: </span>{}</span><span class=\"s\">{}</span></span></a></li>",
            p.route,
            p.step.unwrap(),
            p.step.unwrap(),
            esc(&p.short),
            esc(&p.summary),
        )
        .unwrap();
    }
    o.push_str(
        "</ol><p class=\"self\">This guide is itself a sandboxed component, on your cluster's mock-sites \
         host group: it serves HTTP and imports no files, no sockets and no HTTP client. Its pages \
         were rendered from the READMEs when it was built.</p></nav>",
    );
    o
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// `rel` resolved against the folder `base`, both relative to the repo root.
fn join(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Where a repo path goes: its guide page, or the file or folder on GitHub.
fn repo_target(root: &Path, pages: &[Page], path: &str, cur: &str) -> Option<String> {
    let p = path.trim_end_matches('/');
    let p = p.strip_suffix("README.md").map(|d| d.trim_end_matches('/')).unwrap_or(p);
    if p != cur
        && let Some(page) = pages.iter().find(|pg| pg.dir == p) {
            return Some(page.route.clone());
        }
    let full = root.join(path);
    if full.is_dir() {
        Some(format!("{REPO}/tree/main/{}", path.trim_end_matches('/')))
    } else if full.is_file() {
        Some(format!("{REPO}/blob/main/{path}"))
    } else {
        None
    }
}

fn rewrite_link(root: &Path, pages: &[Page], cur: &str, dest: &str) -> String {
    if dest.contains("://") || dest.starts_with('#') || dest.starts_with("mailto:") {
        return dest.to_string();
    }
    let (path, frag) = match dest.split_once('#') {
        Some((p, f)) => (p, format!("#{f}")),
        None => (dest, String::new()),
    };
    let path = if path.starts_with('/') { join("", path) } else { join(cur, path) };
    match repo_target(root, pages, &path, "") {
        Some(t) => format!("{t}{frag}"),
        None => {
            println!("cargo:warning=guide: link to {dest} in {cur}/README.md goes nowhere");
            dest.to_string()
        }
    }
}

/// Inline code that names a step folder or a repo file: where it should link.
fn code_target(root: &Path, pages: &[Page], cur: &str, s: &str) -> Option<String> {
    const DIRS: [&str; 9] = ["steps", "mock-sites", "k8s", "components", "wit", "scripts", "databases", "artifacts", "docs"];
    const EXTS: [&str; 10] = ["yaml", "yml", "txt", "md", "rs", "wit", "sh", "sql", "toml", "html"];
    if s.is_empty() || s.contains(char::is_whitespace) || s.starts_with(['.', '/', '-']) || s.contains("://") {
        return None;
    }
    let bare = s.trim_end_matches('/');
    let ext_ok = bare.rsplit_once('.').is_some_and(|(_, e)| EXTS.contains(&e));
    let dir_ok = DIRS.contains(&bare.split('/').next().unwrap_or(""));
    if !ext_ok && !(dir_ok && root.join(bare).is_dir()) {
        return None;
    }
    // Relative to this README's folder first, then to the repo root.
    [join(cur, bare), join("", bare)]
        .into_iter()
        .filter(|p| root.join(p).exists())
        .find_map(|p| repo_target(root, pages, &p, cur))
}

/// Whether a browser can open this URL as it stands. Not the ingress address
/// (it needs a Host header), in-cluster names, the metadata service, the
/// GitHub API, or deep paths on *.localhost (the sign-in callback).
fn openable(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let (host, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let name = host.split(':').next().unwrap_or("");
    if name == "localhost" || name.ends_with(".localhost") {
        return path.is_empty() || path == "/";
    }
    !(name.is_empty()
        || name == "127.0.0.1"
        || name.ends_with(".svc")
        || name.starts_with("169.254.")
        || name == "api.github.com")
}

/// Escape text, turning openable URLs into links; in prose, beat labels too.
fn linkify(text: &str, prose: bool, out: &mut String) {
    let mut rest = text;
    loop {
        let at = [rest.find("http://"), rest.find("https://")].into_iter().flatten().min();
        let Some(at) = at else { break };
        plain(&rest[..at], prose, out);
        let tail = &rest[at..];
        let mut end = tail.find(|c: char| c.is_whitespace() || "<>\"'`|".contains(c)).unwrap_or(tail.len());
        while end > 0 && ".,;:!?)*".contains(&tail[..end][end - 1..]) {
            end -= 1;
        }
        let url = &tail[..end];
        if openable(url) {
            write!(out, "<a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a>", esc(url), esc(url)).unwrap();
        } else {
            out.push_str(&esc(url));
        }
        rest = &tail[end..];
    }
    plain(rest, prose, out);
}

fn plain(text: &str, prose: bool, out: &mut String) {
    if !prose {
        out.push_str(&esc(text));
        return;
    }
    let mut rest = text;
    while let Some(at) = ["(allowed", "(refused"].iter().filter_map(|k| rest.find(k)).min() {
        let Some(close) = rest[at..].find(')') else { break };
        let inner = &rest[at + 1..at + close];
        let kind = if inner.starts_with("allowed") { "allowed" } else { "refused" };
        out.push_str(&esc(&rest[..at]));
        write!(out, "<span class=\"badge {kind}\">{}</span>", esc(inner)).unwrap();
        rest = &rest[at + close + 1..];
    }
    out.push_str(&esc(rest));
}

// ---------------------------------------------------------------------------
// One page
// ---------------------------------------------------------------------------

fn render(root: &Path, pages: &[Page], idx: usize) -> String {
    let page = &pages[idx];
    let cur = page.dir.as_str();
    let mut events: Vec<Event> = Vec::new();
    // Join adjacent text, so a URL is never split across two events.
    for ev in Parser::new_ext(&page.md, options()) {
        if let (Event::Text(t), Some(Event::Text(prev))) = (&ev, events.last_mut()) {
            *prev = CowStr::from(format!("{prev}{t}"));
            continue;
        }
        events.push(ev);
    }

    // Heading ids, unique on the page.
    let mut seen = HashSet::new();
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Heading { .. }) = events[i] {
            let mut text = String::new();
            let mut j = i + 1;
            while !matches!(events[j], Event::End(TagEnd::Heading(_))) {
                if let Event::Text(t) | Event::Code(t) = &events[j] {
                    text.push_str(t);
                }
                j += 1;
            }
            let base = slug(&text);
            let mut id = base.clone();
            let mut n = 1;
            while !seen.insert(id.clone()) {
                id = format!("{base}-{n}");
                n += 1;
            }
            if let Event::Start(Tag::Heading { id: slot, .. }) = &mut events[i] {
                *slot = Some(CowStr::from(id));
            }
        }
        i += 1;
    }

    let events = without_codespaces(events, cur);
    let mut out: Vec<Event> = Vec::new();
    let mut in_link = 0;
    let mut code: Option<(&str, String)> = None; // (label, text)
    let mut after_shell = false;
    let mut h2s = 0;
    for ev in events {
        if let Some((_, text)) = &mut code {
            match ev {
                Event::Text(t) => text.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (label, text) = code.take().unwrap();
                    out.push(Event::Html(code_block(label, &text).into()));
                    after_shell = label == "Shell";
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match &kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let label = match lang.as_str() {
                    "sh" | "bash" | "shell" | "console" => "Shell",
                    "chat" => "Chat",
                    "yaml" | "yml" => "YAML",
                    "text" | "txt" | "output" => "Output",
                    _ if after_shell => "Output",
                    _ => "Text",
                };
                code = Some((label, String::new()));
            }
            Event::Start(Tag::Heading { level: HeadingLevel::H2, .. }) if page.route == "/" => {
                h2s += 1;
                // The step list goes after the introduction and Start here,
                // before the next section.
                if h2s == 3 {
                    out.push(Event::Html(step_list(pages).into()));
                }
                after_shell = false;
                out.push(ev);
            }
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                in_link += 1;
                let dest = rewrite_link(root, pages, cur, &dest_url);
                let external = dest.contains("://");
                let tag = Tag::Link { link_type, dest_url: dest.into(), title, id };
                if external {
                    // push_html can't add attributes, so write the tag by hand.
                    if let Tag::Link { dest_url, title, .. } = &tag {
                        let t = if title.is_empty() { String::new() } else { format!(" title=\"{}\"", esc(title)) };
                        out.push(Event::InlineHtml(
                            format!("<a href=\"{}\"{t} target=\"_blank\" rel=\"noopener\">", esc(dest_url)).into(),
                        ));
                    }
                } else {
                    out.push(Event::Start(tag));
                }
            }
            Event::End(TagEnd::Link) => {
                in_link -= 1;
                out.push(Event::InlineHtml("</a>".into()));
            }
            Event::Text(t) if in_link == 0 => {
                let mut html = String::new();
                linkify(&t, true, &mut html);
                out.push(Event::InlineHtml(html.into()));
            }
            Event::Code(c) if in_link == 0 => {
                if let Some(href) = code_target(root, pages, cur, &c) {
                    let ext = if href.contains("://") { " target=\"_blank\" rel=\"noopener\"" } else { "" };
                    out.push(Event::InlineHtml(
                        format!("<a href=\"{}\"{ext}><code{}>{}</code></a>", esc(&href), nw(&c), esc(&c)).into(),
                    ));
                } else {
                    let mut html = format!("<code{}>", nw(&c));
                    linkify(&c, false, &mut html);
                    html.push_str("</code>");
                    out.push(Event::InlineHtml(html.into()));
                }
            }
            Event::Start(Tag::Table(_)) => {
                after_shell = false;
                out.push(Event::Html("<div class=\"table\">".into()));
                out.push(ev);
            }
            Event::End(TagEnd::Table) => {
                out.push(ev);
                out.push(Event::Html("</div>".into()));
            }
            Event::Start(Tag::Paragraph | Tag::List(_) | Tag::Heading { .. } | Tag::BlockQuote(_)) => {
                after_shell = false;
                out.push(ev);
            }
            ev => out.push(ev),
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    html
}

/// The events with each `<!-- codespaces -->` ... `<!-- /codespaces -->`
/// span removed, markers included. Panics (failing the build) on a marker
/// that is nested, unopened or unclosed, so no Codespaces line leaks into the
/// guide and no page text silently disappears.
fn without_codespaces<'a>(events: Vec<Event<'a>>, cur: &str) -> Vec<Event<'a>> {
    let readme = if cur.is_empty() { "README.md".to_string() } else { format!("{cur}/README.md") };
    let mut out = Vec::with_capacity(events.len());
    let mut skipping = false;
    for ev in events {
        let marker = match &ev {
            Event::Html(h) | Event::InlineHtml(h) => match h.trim() {
                CODESPACES_OPEN => Some(true),
                CODESPACES_CLOSE => Some(false),
                t if t.contains("codespaces -->") => panic!("{readme}: a codespaces marker must be alone on its line: {t:?}"),
                _ => None,
            },
            _ => None,
        };
        match marker {
            Some(true) if skipping => panic!("{readme}: {CODESPACES_OPEN} inside another"),
            Some(false) if !skipping => panic!("{readme}: {CODESPACES_CLOSE} with no {CODESPACES_OPEN}"),
            Some(open) => skipping = open,
            None if skipping => {}
            None => out.push(ev),
        }
    }
    assert!(!skipping, "{readme}: {CODESPACES_OPEN} is never closed");
    // A marker is an HTML block of its own: drop the empty blocks left behind.
    let mut kept: Vec<Event<'a>> = Vec::with_capacity(out.len());
    for ev in out {
        if matches!(ev, Event::End(TagEnd::HtmlBlock)) && matches!(kept.last(), Some(Event::Start(Tag::HtmlBlock))) {
            kept.pop();
            continue;
        }
        kept.push(ev);
    }
    kept
}

/// A code block: its label, a Copy button on what the reader sends (Shell,
/// Chat), and the text.
fn code_block(label: &str, text: &str) -> String {
    let class = match label {
        "Output" => " out",
        "Chat" => " chat",
        _ => "",
    };
    let copy = if matches!(label, "Shell" | "Chat") {
        "<button type=\"button\" class=\"copy\">Copy</button>"
    } else {
        ""
    };
    let mut html = format!(
        "<div class=\"code{class}\"><div class=\"code-bar\"><span class=\"code-label\">{label}</span>{copy}</div><pre><code>"
    );
    linkify(text.strip_suffix('\n').unwrap_or(text), false, &mut html);
    html.push_str("</code></pre></div>\n");
    html
}

/// Short code with no spaces (a folder, a flag, a file) shouldn't break at a hyphen.
fn nw(code: &str) -> &'static str {
    if code.len() <= 28 && !code.contains(char::is_whitespace) { " class=\"nw\"" } else { "" }
}

/// POSIX cksum: CRC-32 (polynomial 0x04C11DB7, most significant bit first)
/// over the bytes and then the length, complemented. Matches `cksum <file>`.
fn cksum(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    let mut step = |b: u8| {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04C1_1DB7 } else { crc << 1 };
        }
    };
    data.iter().for_each(|&b| step(b));
    let mut len = data.len();
    while len > 0 {
        step((len & 0xff) as u8);
        len >>= 8;
    }
    !crc
}
