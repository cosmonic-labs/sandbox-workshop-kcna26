/// Operator-supplied addresses for the two workshop mock sites.
/// The host checks the rewritten destination against allowedHosts on each hop.
pub fn target(url: &str, api: Option<&str>, attacker: Option<&str>) -> String {
    let Some(rest) = url.strip_prefix("http://") else {
        return url.into();
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let base = match host {
        "api.mock-sites.svc" => api,
        "attacker.mock-sites.svc" => attacker,
        _ => None,
    };
    match base {
        Some(base) => format!("{}/{path}", base.trim_end_matches('/')),
        None => url.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_mock_hosts_are_mapped() {
        let api = Some("http://127.0.0.1:28191");
        let attacker = Some("http://127.0.0.1:28192");
        assert_eq!(
            target("http://api.mock-sites.svc/data?q=1", api, attacker),
            "http://127.0.0.1:28191/data?q=1"
        );
        assert_eq!(
            target("http://attacker.mock-sites.svc/collect", api, attacker),
            "http://127.0.0.1:28192/collect"
        );
        for url in [
            "http://api.mock-sites.svc.evil/data",
            "http://169.254.169.254/",
            "https://api.github.com/repos",
            "file:///etc/passwd",
        ] {
            assert_eq!(target(url, api, attacker), url);
        }
    }

    #[test]
    fn without_a_mapping_the_cluster_address_is_unchanged() {
        let url = "http://api.mock-sites.svc/data";
        assert_eq!(target(url, None, None), url);
    }
}
