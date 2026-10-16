use url::{Host, Url};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedUrl {
    value: String,
    host: String,
}

impl NormalizedUrl {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Err(Error::EmptyUrl);
        }
        if input.chars().any(char::is_control) || input.chars().any(char::is_whitespace) {
            return Err(Error::InvalidUrl);
        }
        let parsed = if input.contains("://") || has_non_port_scheme(input) {
            Url::parse(input).map_err(|_| Error::InvalidUrl)?
        } else {
            parse_bare(input)?
        };
        from_url(parsed)
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

fn has_non_port_scheme(input: &str) -> bool {
    let Some((prefix, suffix)) = input.split_once(':') else {
        return false;
    };
    let mut chars = prefix.chars();
    let valid_scheme = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|value| value.is_ascii_alphanumeric() || matches!(value, '+' | '-' | '.'));
    valid_scheme && !suffix.chars().all(|value| value.is_ascii_digit())
}

fn parse_bare(input: &str) -> Result<Url> {
    let probe = match Url::parse(&format!("http://{input}")) {
        Ok(value) => value,
        // Not a bare host: try the input as a full URL.
        Err(_) => return Url::parse(input).map_err(|_| Error::InvalidUrl),
    };
    let local_with_port = probe.port().is_some() && is_loopback(probe.host());
    let scheme = if local_with_port { "http" } else { "https" };
    Url::parse(&format!("{scheme}://{input}")).map_err(|_| Error::InvalidUrl)
}

fn from_url(url: Url) -> Result<NormalizedUrl> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::UnsupportedScheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidUrl);
    }
    let host = url.host_str().ok_or(Error::MissingHost)?.to_owned();
    Ok(NormalizedUrl {
        value: url.into(),
        host,
    })
}

fn is_loopback(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain(value)) => value.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(value)) => value.is_loopback(),
        Some(Host::Ipv6(value)) => value.is_loopback(),
        None => false,
    }
}
