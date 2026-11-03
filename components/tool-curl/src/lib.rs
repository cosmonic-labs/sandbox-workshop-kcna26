//! curl: HTTP requests from a command line.
//!
//! curl's famous escapes are reaching places it shouldn't: the cloud
//! metadata service, the machine's own loopback services, `file://` URLs,
//! and writing anywhere with `-o`. Here every request goes through
//! wasi:http, where the host checks it against this component's
//! allowedHosts before anything leaves, and files exist only in the folder
//! the manifest mounts for it. The boundary is the manifest, not this code.

wit_bindgen::generate!({
    path: "../../wit",
    world: "workshop:toolbox/curl-tool",
    generate_all,
});

#[path = "../../common/http_client.rs"]
mod http_client;
mod cli;

use exports::workshop::toolbox::curl::{Error, Guest, RunArgs};
use http_client::SendError;
use wasi::http::types::ErrorCode;

/// The folder relative paths in `-o` and `-d @file` resolve against.
const FOLDER: &str = "/data";

fn in_folder(path: &str) -> String {
    if path.starts_with('/') { path.to_string() } else { format!("{FOLDER}/{path}") }
}

/// Why a request produced no response, as this tool's typed error, in the
/// words curl would use.
fn send_error(url: &str, e: SendError) -> Error {
    let host = cli::host_of(url);
    match e {
        SendError::Invalid(m) => Error::InvalidArgument(m),
        SendError::Http(ErrorCode::HttpRequestDenied) => Error::AccessDenied(format!(
            "{host}: this tool may not reach that host (not in its allowedHosts)"
        )),
        SendError::Http(ErrorCode::DnsError(_) | ErrorCode::DnsTimeout) => {
            Error::Network(format!("Could not resolve host: {host}"))
        }
        SendError::Http(ErrorCode::ConnectionRefused) => {
            Error::Network(format!("Failed to connect to {host}: Connection refused"))
        }
        SendError::Http(
            code @ (ErrorCode::ConnectionTimeout
            | ErrorCode::ConnectionReadTimeout
            | ErrorCode::ConnectionWriteTimeout
            | ErrorCode::ConnectionTerminated
            | ErrorCode::DestinationNotFound
            | ErrorCode::DestinationUnavailable
            | ErrorCode::DestinationIpProhibited
            | ErrorCode::DestinationIpUnroutable
            | ErrorCode::TlsProtocolError
            | ErrorCode::TlsCertificateError
            | ErrorCode::TlsAlertReceived(_)),
        ) => Error::Network(format!("{host}: {code:?}")),
        SendError::Http(code) => Error::Other(Some(format!("{host}: {code:?}"))),
    }
}

/// A file error for `path`, as this tool's typed error.
fn file_error(path: &str, e: std::io::Error) -> Error {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound | K::PermissionDenied | K::ReadOnlyFilesystem => {
            Error::AccessDenied(format!("{path}: {e} (this tool sees only {FOLDER})"))
        }
        _ => Error::Other(Some(format!("{path}: {e}"))),
    }
}

async fn run(o: cli::Opts) -> Result<String, Error> {
    let first = o.url.as_deref().ok_or_else(|| Error::InvalidArgument("no URL specified".into()))?;
    let mut url = cli::check_scheme(first).map_err(Error::InvalidArgument)?;
    let mut body = match &o.data_file {
        Some(p) => {
            let path = in_folder(p);
            Some(std::fs::read(&path).map_err(|e| file_error(&path, e))?)
        }
        None => o.data.clone().map(String::into_bytes),
    };
    let mut method = match (&o.method, &body) {
        (Some(m), _) => m.to_ascii_uppercase(),
        (None, Some(_)) => "POST".into(),
        (None, None) => "GET".into(),
    };
    let mut out = String::new();
    let mut hops = 0;
    loop {
        let mut headers: Vec<(&str, &str)> =
            o.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        headers.push(("user-agent", "curl/8 (workshop component)"));
        if body.is_some() && !o.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            headers.push(("content-type", "application/x-www-form-urlencoded"));
        }
        if o.verbose {
            out.push_str(&format!("> {method} {url}\n"));
            for (k, v) in &headers {
                out.push_str(&format!("> {k}: {v}\n"));
            }
        }
        let (status, resp_headers, bytes) =
            http_client::send_full(&method, &url, &headers, body.clone().unwrap_or_default())
                .await
                .map_err(|e| send_error(&url, e))?;
        if o.include || o.verbose {
            let prefix = if o.verbose { "< " } else { "" };
            out.push_str(&format!("{prefix}HTTP/1.1 {status}\n"));
            for (k, v) in &resp_headers {
                out.push_str(&format!("{prefix}{k}: {}\n", String::from_utf8_lossy(v)));
            }
            out.push('\n');
        }
        let location = resp_headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("location"))
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned());
        if o.location && (300..400).contains(&status) {
            if let Some(loc) = location {
                hops += 1;
                if hops > o.max_redirs {
                    return Err(Error::Other(Some(format!(
                        "Maximum ({}) redirects followed",
                        o.max_redirs
                    ))));
                }
                // Each hop is a new request, checked by the host the same way.
                url = cli::check_scheme(&cli::resolve_location(&url, &loc))
                    .map_err(Error::InvalidArgument)?;
                if matches!(status, 301 | 302 | 303) && method == "POST" {
                    method = "GET".into();
                    body = None;
                }
                continue;
            }
        }
        if o.fail && status >= 400 {
            return Err(Error::Other(Some(format!("The requested URL returned error: {status}"))));
        }
        match &o.output {
            Some(p) if p != "-" => {
                let path = in_folder(p);
                std::fs::write(&path, &bytes).map_err(|e| file_error(&path, e))?;
                out.push_str(&format!("saved {} bytes to {path}\n", bytes.len()));
            }
            _ => out.push_str(&String::from_utf8_lossy(&bytes)),
        }
        return Ok(out);
    }
}

struct Curl;

impl Guest for Curl {
    async fn run(args: RunArgs) -> Result<String, Error> {
        let opts = cli::parse(&args.arguments).map_err(Error::InvalidArgument)?;
        run(opts).await
    }
}

export!(Curl);
