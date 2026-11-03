//! curl-wasm: a curl-compatible subset whose only network path is wasi:http.
//!
//! Scheme rejection below is this tool's own code. The real boundary is that
//! the component imports wasi:http/outgoing-handler and no wasi:sockets, and
//! the host decides which hosts an outgoing request may reach.

use std::io::Write;
use std::process::exit;
use wstd::http::{Body, BodyExt, Client, ErrorCode, Method, Request, Uri};

struct Opts {
    url: Option<String>,
    method: Option<String>,
    headers: Vec<(String, String)>,
    data: Option<Vec<u8>>,
    include: bool,
    silent: bool,
    show_error: bool,
    verbose: bool,
    output: Option<String>,
    location: bool,
    fail: bool,
    max_redirs: u32,
}

/// curl-style failure: exit code plus message.
struct Fail(i32, String);

fn usage() -> ! {
    eprintln!(
        "Usage: curl [options...] <url>\n \
         -X, --request <method>  -H, --header <h: v>  -d, --data <data|@file>\n \
         -i, --include  -s, --silent  -S, --show-error  -v, --verbose\n \
         -o, --output <file>  -L, --location  -f, --fail  --max-redirs <n>"
    );
    exit(2)
}

fn parse_args() -> Result<Opts, Fail> {
    let mut o = Opts {
        url: None,
        method: None,
        headers: vec![],
        data: None,
        include: false,
        silent: false,
        show_error: false,
        verbose: false,
        output: None,
        location: false,
        fail: false,
        max_redirs: 50,
    };
    // A wasmCloud service gets no argv, so fall back to CURL_ARGS
    // (whitespace-split) from wasi:cli/environment when argv is empty.
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        if let Ok(env_args) = std::env::var("CURL_ARGS") {
            argv = env_args.split_whitespace().map(String::from).collect();
        }
    }
    let mut args = argv.into_iter();
    while let Some(a) = args.next() {
        let mut val = |name: &str| {
            args.next()
                .ok_or_else(|| Fail(2, format!("option {name}: requires parameter")))
        };
        match a.as_str() {
            "-X" | "--request" => o.method = Some(val(&a)?),
            "-H" | "--header" => {
                let h = val(&a)?;
                let (k, v) = h
                    .split_once(':')
                    .ok_or_else(|| Fail(2, format!("bad header: {h}")))?;
                o.headers.push((k.trim().to_string(), v.trim().to_string()));
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                let d = val(&a)?;
                let bytes = match d.strip_prefix('@') {
                    // Reads go through wasi:filesystem, so only preopened dirs work.
                    Some(path) if a != "--data-raw" => std::fs::read(path).map_err(|e| {
                        Fail(26, format!("Failed to open/read local data from file {path}: {e}"))
                    })?,
                    _ => d.into_bytes(),
                };
                let buf = o.data.get_or_insert_with(Vec::new);
                if !buf.is_empty() {
                    buf.push(b'&');
                }
                buf.extend(bytes);
            }
            "-i" | "--include" => o.include = true,
            "-s" | "--silent" => o.silent = true,
            "-S" | "--show-error" => o.show_error = true,
            "-sS" | "-Ss" => {
                o.silent = true;
                o.show_error = true
            }
            "-v" | "--verbose" => o.verbose = true,
            "-o" | "--output" => o.output = Some(val(&a)?),
            "-L" | "--location" => o.location = true,
            "-f" | "--fail" => o.fail = true,
            "--max-redirs" => {
                o.max_redirs = val(&a)?
                    .parse()
                    .map_err(|_| Fail(2, "bad --max-redirs".into()))?
            }
            "-h" | "--help" => usage(),
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(Fail(2, format!("option {s}: is unknown")))
            }
            _ => o.url = Some(a),
        }
    }
    Ok(o)
}

/// Only http and https are wired up. Everything else is refused here, before any
/// request exists. This is a convenience list, not the security boundary.
fn check_scheme(url: &str) -> Result<String, Fail> {
    let url = if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{url}") // curl's default when no scheme is given
    };
    let scheme = url.split("://").next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" => Ok(url),
        _ => Err(Fail(
            1,
            format!("Protocol \"{scheme}\" not supported or disabled in libcurl"),
        )),
    }
}

fn resolve_location(base: &Uri, loc: &str) -> String {
    let scheme = base.scheme_str().unwrap_or("http");
    let auth = base.authority().map(|a| a.as_str()).unwrap_or("");
    if loc.contains("://") {
        loc.to_string()
    } else if let Some(rest) = loc.strip_prefix("//") {
        format!("{scheme}://{rest}")
    } else if loc.starts_with('/') {
        format!("{scheme}://{auth}{loc}")
    } else {
        let path = base.path();
        let dir = &path[..path.rfind('/').map(|i| i + 1).unwrap_or(0)];
        format!("{scheme}://{auth}{dir}{loc}")
    }
}

fn describe(e: &wstd::http::Error, host: &str) -> Fail {
    let Fail(code, msg) = describe_inner(e, host);
    match e.downcast_ref::<ErrorCode>() {
        // Show the raw wasi:http error-code so host refusals and network
        // failures are distinguishable.
        Some(c) => Fail(code, format!("{msg} [wasi:http error-code: {c:?}]")),
        None => Fail(code, msg),
    }
}

fn describe_inner(e: &wstd::http::Error, host: &str) -> Fail {
    match e.downcast_ref::<ErrorCode>() {
        Some(ErrorCode::DnsError(_)) => Fail(6, format!("Could not resolve host: {host}")),
        Some(ErrorCode::ConnectionRefused) => Fail(7, format!("Failed to connect to {host}")),
        Some(ErrorCode::ConnectionTimeout) => Fail(28, format!("Connection timed out: {host}")),
        Some(ErrorCode::HttpRequestDenied) => {
            Fail(7, format!("Request to {host} denied by the host (wasi:http)"))
        }
        Some(ErrorCode::TlsCertificateError) | Some(ErrorCode::TlsAlertReceived(_)) => {
            Fail(60, format!("SSL certificate problem: {host}"))
        }
        Some(c) => Fail(7, format!("wasi:http error for {host}: {c:?}")),
        None => Fail(7, format!("request failed for {host}: {e:#}")),
    }
}

async fn run(o: Opts) -> Result<(), Fail> {
    let mut url = check_scheme(o.url.as_deref().ok_or_else(|| Fail(2, "no URL specified".into()))?)?;
    let mut method = match (&o.method, &o.data) {
        (Some(m), _) => m.to_ascii_uppercase(),
        (None, Some(_)) => "POST".into(),
        (None, None) => "GET".into(),
    };
    let mut body = o.data.clone();
    let client = Client::new();
    let mut out: Vec<u8> = Vec::new();
    let mut hops = 0;

    loop {
        let uri: Uri = url
            .parse()
            .map_err(|e| Fail(3, format!("URL using bad/illegal format: {e}")))?;
        let host = uri.host().unwrap_or("").to_string();
        let m = Method::from_bytes(method.as_bytes())
            .map_err(|_| Fail(2, format!("bad method {method}")))?;

        let mut req = Request::builder().method(m).uri(uri.clone());
        let mut has_ct = false;
        for (k, v) in &o.headers {
            has_ct |= k.eq_ignore_ascii_case("content-type");
            req = req.header(k.as_str(), v.as_str());
        }
        req = req.header("user-agent", "curl-wasm/0.1 (wasi:http)");
        if let Some(b) = &body {
            req = req.header("content-length", b.len().to_string());
        }
        if body.is_some() && !has_ct {
            req = req.header("content-type", "application/x-www-form-urlencoded");
        }
        let req = req
            .body(Body::from(body.clone().unwrap_or_default()))
            .map_err(|e| Fail(2, format!("bad request: {e}")))?;

        if o.verbose {
            eprintln!("> {} {} (wasi:http outgoing-handler)", req.method(), url);
            for (k, v) in req.headers() {
                eprintln!("> {k}: {}", String::from_utf8_lossy(v.as_bytes()));
            }
        }

        let resp = client.send(req).await.map_err(|e| describe(&e, &host))?;
        let status = resp.status();

        if o.include || o.verbose {
            let mut head = format!(
                "HTTP/1.1 {} {}\r\n",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            );
            for (k, v) in resp.headers() {
                head += &format!("{k}: {}\r\n", String::from_utf8_lossy(v.as_bytes()));
            }
            head += "\r\n";
            if o.verbose {
                for l in head.lines() {
                    eprintln!("< {l}");
                }
            }
            if o.include {
                out.extend(head.as_bytes());
            }
        }

        let location = resp
            .headers()
            .get("location")
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());

        if o.location && status.is_redirection() {
            if let Some(loc) = location {
                hops += 1;
                if hops > o.max_redirs {
                    return Err(Fail(47, format!("Maximum ({}) redirects followed", o.max_redirs)));
                }
                // Each hop is a new wasi:http request, checked by the same rules.
                url = check_scheme(&resolve_location(&uri, &loc))?;
                if matches!(status.as_u16(), 301 | 302 | 303) && method == "POST" {
                    method = "GET".into();
                    body = None;
                }
                continue;
            }
        }

        if o.fail && status.as_u16() >= 400 {
            return Err(Fail(
                22,
                format!("The requested URL returned error: {}", status.as_u16()),
            ));
        }

        let bytes = resp
            .into_body()
            .into_boxed_body()
            .collect()
            .await
            .map_err(|e| Fail(56, format!("Failure when receiving data from the peer: {e}")))?
            .to_bytes();
        out.extend_from_slice(&bytes);
        break;
    }

    match &o.output {
        // Writes go through wasi:filesystem, so only preopened dirs work.
        Some(path) if path != "-" => std::fs::write(path, &out)
            .map_err(|e| Fail(23, format!("Failure writing output to destination ({path}): {e}"))),
        _ => std::io::stdout()
            .write_all(&out)
            .map_err(|e| Fail(23, format!("Failure writing output: {e}"))),
    }
}

fn main() {
    let o = match parse_args() {
        Ok(o) => o,
        Err(Fail(c, m)) => {
            eprintln!("curl: {m}");
            exit(c)
        }
    };
    let quiet = o.silent && !o.show_error;
    if let Err(Fail(code, msg)) = wstd::runtime::block_on(run(o)) {
        if !quiet {
            eprintln!("curl: ({code}) {msg}");
        }
        exit(code);
    }
}
