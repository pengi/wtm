//! HTTP GET requests, made by running `curl`.
//!
//! Running curl keeps TLS out of the wtm binary: the Rust TLS stacks all
//! include Apache-2.0-only code, which can't be combined with wtm's
//! GPL-2.0-only licence. It also means the system's CA certificates and
//! proxy settings apply.

use std::io::Write;
use std::process::{Command, Stdio};

use super::UpstreamError;

pub struct Response {
    pub status: u16,
    /// Headers of the final response, names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The `rel="next"` address of a `Link` header, for paginated results.
    pub fn next_link(&self) -> Option<String> {
        self.header("link")?.split(',').find_map(|part| {
            let (url, params) = part.split_once(';')?;
            params.contains("rel=\"next\"").then(|| {
                url.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string()
            })
        })
    }
}

pub trait HttpClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, UpstreamError>;
}

/// Makes requests with the `curl` command.
pub struct CurlClient;

impl HttpClient for CurlClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, UpstreamError> {
        let header_file = tempfile::NamedTempFile::new().map_err(UpstreamError::Spawn)?;

        // Everything goes through a config on standard input, so tokens in
        // headers never appear on a command line.
        let mut config = String::new();
        config_line(&mut config, "url", url);
        for (name, value) in headers {
            config_line(&mut config, "header", &format!("{name}: {value}"));
        }
        config_line(
            &mut config,
            "user-agent",
            concat!("wtm/", env!("CARGO_PKG_VERSION")),
        );
        config_line(
            &mut config,
            "dump-header",
            &header_file.path().to_string_lossy(),
        );
        config.push_str("write-out = \"\\n%{http_code}\"\n");
        config.push_str("silent\nshow-error\nlocation\n");

        let mut child = Command::new("curl")
            .args(["--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(UpstreamError::Spawn)?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config.as_bytes())
                .map_err(UpstreamError::Spawn)?;
        }
        let out = child.wait_with_output().map_err(UpstreamError::Spawn)?;

        let local = url.starts_with("file://");
        if !out.status.success() {
            // curl's "couldn't read file" for file:// addresses.
            if local && out.status.code() == Some(37) {
                return Ok(Response {
                    status: 404,
                    headers: Vec::new(),
                    body: Vec::new(),
                });
            }
            return Err(UpstreamError::Transport {
                url: url.to_string(),
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }

        let mut body = out.stdout;
        let newline = body.iter().rposition(|b| *b == b'\n').unwrap_or(0);
        let code = String::from_utf8_lossy(&body[newline..]).trim().to_string();
        body.truncate(newline);
        let mut status = code.parse().unwrap_or(0);
        // file:// has no status; a file that was read is a success.
        if status == 0 && local {
            status = 200;
        }

        let raw = std::fs::read_to_string(header_file.path()).unwrap_or_default();
        Ok(Response {
            status,
            headers: parse_headers(&raw),
            body,
        })
    }
}

fn config_line(config: &mut String, key: &str, value: &str) {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], "");
    config.push_str(&format!("{key} = \"{escaped}\"\n"));
}

/// Headers of the last response in a dump that may contain several
/// (redirects).
fn parse_headers(raw: &str) -> Vec<(String, String)> {
    let raw = raw.replace("\r\n", "\n");
    let last = raw
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .last()
        .unwrap_or("");
    last.lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

/// Percent-encode everything but unreserved characters (RFC 3986).
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_header_block_wins() {
        let raw = "HTTP/1.1 301 Moved\r\nLocation: x\r\n\r\nHTTP/2 200\r\nLink: <a>; rel=\"next\"\r\n\r\n";
        let headers = parse_headers(raw);
        assert_eq!(
            headers,
            vec![("link".to_string(), "<a>; rel=\"next\"".to_string())]
        );
    }

    #[test]
    fn next_link() {
        let response = Response {
            status: 200,
            headers: vec![(
                "link".into(),
                "<https://x/p?page=1>; rel=\"prev\", <https://x/p?page=3>; rel=\"next\"".into(),
            )],
            body: Vec::new(),
        };
        assert_eq!(response.next_link().as_deref(), Some("https://x/p?page=3"));
    }

    #[test]
    fn encoding() {
        assert_eq!(encode("group/sub project"), "group%2Fsub%20project");
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn config_values_are_escaped() {
        let mut config = String::new();
        config_line(&mut config, "header", "a\"b\\c\nd");
        assert_eq!(config, "header = \"a\\\"b\\\\cd\"\n");
    }
}
