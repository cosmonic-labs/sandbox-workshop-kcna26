//! curl's command line, parsed. No I/O here, so it tests natively:
//! `rustc --edition 2024 --test src/cli.rs -o /tmp/cli && /tmp/cli`.

/// What a curl command line asks for.
#[derive(Debug, Default, PartialEq)]
pub struct Opts {
    pub url: Option<String>,
    pub method: Option<String>,
    pub headers: Vec<(String, String)>,
    pub data: Option<String>,
    pub data_file: Option<String>,
    pub include: bool,
    pub verbose: bool,
    pub output: Option<String>,
    pub location: bool,
    pub fail: bool,
    pub max_redirs: u32,
}

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

/// Split a bundle of short flags, as curl does: `-sL` becomes `-s -L`. A
/// token is a bundle when it is a single dash followed by two or more letters.
/// The last flag in a bundle may still take the next argument as its value, so
/// `-so out.txt` becomes `-s -o out.txt`.
fn expand_bundles(args: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        let is_bundle = a.len() > 2
            && a.starts_with('-')
            && !a.starts_with("--")
            && a[1..].chars().all(|c| c.is_ascii_alphabetic());
        if is_bundle {
            for c in a[1..].chars() {
                out.push(format!("-{c}"));
            }
        } else {
            out.push(a);
        }
    }
    out
}

/// Parse curl's options. Unknown options are an error, as in curl.
pub fn parse(line: &str) -> Result<Opts, String> {
    let mut o = Opts { max_redirs: 50, ..Default::default() };
    let mut args = expand_bundles(split(line)?).into_iter();
    while let Some(a) = args.next() {
        let mut val = |name: &str| args.next().ok_or_else(|| format!("option {name}: requires parameter"));
        match a.as_str() {
            "curl" if o.url.is_none() && o.headers.is_empty() => {} // tolerate a leading "curl"
            "-X" | "--request" => o.method = Some(val(&a)?),
            "-H" | "--header" => {
                let h = val(&a)?;
                let (k, v) = h.split_once(':').ok_or_else(|| format!("bad header: {h}"))?;
                o.headers.push((k.trim().to_string(), v.trim().to_string()));
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                let d = val(&a)?;
                match d.strip_prefix('@') {
                    Some(path) if a != "--data-raw" => o.data_file = Some(path.to_string()),
                    _ => {
                        let buf = o.data.get_or_insert_with(String::new);
                        if !buf.is_empty() {
                            buf.push('&');
                        }
                        buf.push_str(&d);
                    }
                }
            }
            "-i" | "--include" => o.include = true,
            "-s" | "--silent" | "-S" | "--show-error" => {}
            "-v" | "--verbose" => o.verbose = true,
            "-o" | "--output" => o.output = Some(val(&a)?),
            "-L" | "--location" => o.location = true,
            "-f" | "--fail" => o.fail = true,
            "--max-redirs" => {
                o.max_redirs = val(&a)?.parse().map_err(|_| "bad --max-redirs".to_string())?
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("option {s}: is unknown")),
            _ => o.url = Some(a),
        }
    }
    Ok(o)
}

/// Add curl's default scheme, and refuse schemes other than http and https.
/// A convenience, not the boundary: the host decides which hosts are
/// reachable.
pub fn check_scheme(url: &str) -> Result<String, String> {
    let url = if url.contains("://") { url.to_string() } else { format!("http://{url}") };
    let scheme = url.split("://").next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" => Ok(url),
        _ => Err(format!("Protocol \"{scheme}\" not supported or disabled in libcurl")),
    }
}

/// The URL a redirect's Location header points at, relative to `base`.
pub fn resolve_location(base: &str, loc: &str) -> String {
    let (scheme, rest) = base.split_once("://").unwrap_or(("http", base));
    let (auth, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if loc.contains("://") {
        loc.to_string()
    } else if let Some(r) = loc.strip_prefix("//") {
        format!("{scheme}://{r}")
    } else if loc.starts_with('/') {
        format!("{scheme}://{auth}{loc}")
    } else {
        let path = path.split('?').next().unwrap_or("/");
        let dir = &path[..path.rfind('/').map(|i| i + 1).unwrap_or(0)];
        format!("{scheme}://{auth}{dir}{loc}")
    }
}

/// The host part of a URL, for messages.
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let auth = rest.split(['/', '?']).next().unwrap_or(rest);
    auth.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_shell() {
        assert_eq!(
            split(r#"-s -H "Accept: application/json" 'https://x.test/a b'"#).unwrap(),
            ["-s", "-H", "Accept: application/json", "https://x.test/a b"]
        );
        assert_eq!(split(r#"-d "a\"b""#).unwrap(), ["-d", "a\"b"]);
        assert!(split("\"open").is_err());
    }

    #[test]
    fn parses_curl_options() {
        let o = parse("curl -sS -i -L -X put -H 'A: 1' -d x=1 -d y=2 -o out.txt example.com").unwrap();
        assert!(o.include && o.location);
        assert_eq!(o.method.as_deref(), Some("put"));
        assert_eq!(o.headers, [("A".into(), "1".into())]);
        assert_eq!(o.data.as_deref(), Some("x=1&y=2"));
        assert_eq!(o.output.as_deref(), Some("out.txt"));
        assert_eq!(o.url.as_deref(), Some("example.com"));
        assert_eq!(parse("-d @/data/body.json u").unwrap().data_file.as_deref(), Some("/data/body.json"));
        assert!(parse("--upload-file x u").unwrap_err().contains("unknown"));
        assert!(parse("-H").unwrap_err().contains("requires parameter"));
    }

    #[test]
    fn expands_bundled_short_flags() {
        let o = parse("-sL http://x.test/r").unwrap();
        assert!(o.location);
        assert_eq!(o.url.as_deref(), Some("http://x.test/r"));
        // The last flag in a bundle still takes its value.
        let o = parse("-so out.txt http://x.test").unwrap();
        assert_eq!(o.output.as_deref(), Some("out.txt"));
    }

    #[test]
    fn schemes_and_redirects() {
        assert_eq!(check_scheme("example.com").unwrap(), "http://example.com");
        assert!(check_scheme("file:///etc/passwd").unwrap_err().contains("\"file\""));
        assert!(check_scheme("gopher://x").is_err());
        assert_eq!(resolve_location("https://a.test/x/y?q", "z"), "https://a.test/x/z");
        assert_eq!(resolve_location("https://a.test/x/y", "/r"), "https://a.test/r");
        assert_eq!(resolve_location("https://a.test/x", "//b.test/p"), "https://b.test/p");
        assert_eq!(host_of("http://u:p@169.254.169.254/latest"), "169.254.169.254");
    }
}
