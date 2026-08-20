//! HTTP Digest authentication (RFC 7616 / RFC 2617), MD5 with `qop=auth`.
//!
//! Needed because some WebDAV hosts advertise nothing else: SabreDAV-based servers
//! (BigCommerce among them) answer `WWW-Authenticate: Digest realm="SabreDAV"` and
//! reject Basic outright.
//!
//! MD5 is cryptographically weak. It is not a choice made here — the server dictates
//! the algorithm, and the alternative is no access at all. Everything in this module
//! is a pure function so the whole exchange is testable against the RFC's own vectors.

use md5::{Digest, Md5};

/// A parsed `WWW-Authenticate: Digest ...` challenge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// `auth`, `auth-int`, or absent (legacy RFC 2069 mode).
    pub qop: Option<String>,
    /// `MD5` or `MD5-sess`; anything else is unsupported.
    pub algorithm: Option<String>,
}

fn md5_hex(input: &str) -> String {
    let mut h = Md5::new();
    h.update(input.as_bytes());
    h.finalize().iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Splits a challenge's comma-separated `key=value` list, honouring quotes.
///
/// Values may legitimately contain commas inside quotes (`qop="auth,auth-int"`), so
/// a plain `split(',')` corrupts them.
fn split_params(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut key = String::new();
    let mut val = String::new();
    let mut in_key = true;
    let mut quoted = false;

    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            '=' if in_key && !quoted => in_key = false,
            ',' if !quoted => {
                if !key.trim().is_empty() {
                    out.push((key.trim().to_ascii_lowercase(), val.trim().to_string()));
                }
                key.clear();
                val.clear();
                in_key = true;
            }
            _ => {
                if in_key {
                    key.push(c);
                } else {
                    val.push(c);
                }
            }
        }
    }
    if !key.trim().is_empty() {
        out.push((key.trim().to_ascii_lowercase(), val.trim().to_string()));
    }
    out
}

/// Extracts the Digest challenge from a `WWW-Authenticate` header value.
///
/// Returns `None` when the header advertises no Digest scheme at all. A header may
/// list several schemes (`Basic realm="x", Digest realm="y"`), so the Digest part is
/// located rather than assumed to start at the beginning.
pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let lower = header.to_ascii_lowercase();
    let start = lower.find("digest ")?;
    let params = split_params(&header[start + "digest ".len()..]);

    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    let nonce = get("nonce")?;
    Some(Challenge {
        realm: get("realm").unwrap_or_default(),
        nonce,
        opaque: get("opaque"),
        qop: get("qop"),
        algorithm: get("algorithm"),
    })
}

impl Challenge {
    /// True when this challenge can actually be answered. `SHA-256` variants and
    /// `auth-int` (which would require hashing the body) are not implemented.
    pub fn is_supported(&self) -> bool {
        let algo_ok = match self.algorithm.as_deref() {
            None => true,
            Some(a) => {
                let a = a.to_ascii_uppercase();
                a == "MD5" || a == "MD5-SESS"
            }
        };
        let qop_ok = match self.qop.as_deref() {
            None => true,
            Some(q) => q.split(',').any(|v| v.trim().eq_ignore_ascii_case("auth")),
        };
        algo_ok && qop_ok
    }

    /// The `qop` value to echo back, if the server offered one we support.
    fn chosen_qop(&self) -> Option<&'static str> {
        match self.qop.as_deref() {
            Some(q) if q.split(',').any(|v| v.trim().eq_ignore_ascii_case("auth")) => Some("auth"),
            _ => None,
        }
    }
}

/// Builds the `Authorization: Digest ...` header value.
///
/// `uri` is the request-target as sent on the wire (path + query), `cnonce` and `nc`
/// are supplied by the caller so this stays pure and testable.
pub fn authorization_header(
    challenge: &Challenge,
    user: &str,
    password: &str,
    method: &str,
    uri: &str,
    cnonce: &str,
    nc: u32,
) -> String {
    let ha1_base = md5_hex(&format!("{}:{}:{}", user, challenge.realm, password));
    // MD5-sess folds the nonces into HA1 so the session key changes per challenge.
    let ha1 = if challenge
        .algorithm
        .as_deref()
        .map(|a| a.eq_ignore_ascii_case("MD5-sess"))
        .unwrap_or(false)
    {
        md5_hex(&format!("{}:{}:{}", ha1_base, challenge.nonce, cnonce))
    } else {
        ha1_base
    };
    let ha2 = md5_hex(&format!("{method}:{uri}"));

    let nc_hex = format!("{nc:08x}");
    let qop = challenge.chosen_qop();
    let response = match qop {
        Some(q) => md5_hex(&format!(
            "{}:{}:{}:{}:{}:{}",
            ha1, challenge.nonce, nc_hex, cnonce, q, ha2
        )),
        // RFC 2069 legacy form, for servers that send no qop.
        None => md5_hex(&format!("{}:{}:{}", ha1, challenge.nonce, ha2)),
    };

    let mut parts = vec![
        format!("username=\"{user}\""),
        format!("realm=\"{}\"", challenge.realm),
        format!("nonce=\"{}\"", challenge.nonce),
        format!("uri=\"{uri}\""),
        format!("response=\"{response}\""),
    ];
    if let Some(algo) = &challenge.algorithm {
        parts.push(format!("algorithm={algo}"));
    }
    if let Some(q) = qop {
        parts.push(format!("qop={q}"));
        parts.push(format!("nc={nc_hex}"));
        parts.push(format!("cnonce=\"{cnonce}\""));
    }
    if let Some(opaque) = &challenge.opaque {
        parts.push(format!("opaque=\"{opaque}\""));
    }
    format!("Digest {}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_known_vectors() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn parses_a_sabredav_challenge() {
        // The real header from a BigCommerce store.
        let c = parse_challenge(
            r#"Digest realm="SabreDAV",qop="auth",nonce="6a86ecabd683c",opaque="df58bdff8cf6""#,
        )
        .unwrap();
        assert_eq!(c.realm, "SabreDAV");
        assert_eq!(c.nonce, "6a86ecabd683c");
        assert_eq!(c.qop.as_deref(), Some("auth"));
        assert_eq!(c.opaque.as_deref(), Some("df58bdff8cf6"));
        assert_eq!(c.algorithm, None);
        assert!(c.is_supported());
    }

    #[test]
    fn finds_digest_when_listed_after_basic() {
        let c = parse_challenge(r#"Basic realm="x", Digest realm="y", nonce="n""#).unwrap();
        assert_eq!(c.realm, "y");
        assert_eq!(c.nonce, "n");
    }

    #[test]
    fn returns_none_without_a_digest_scheme() {
        assert!(parse_challenge(r#"Basic realm="x""#).is_none());
        assert!(parse_challenge("").is_none());
    }

    #[test]
    fn a_challenge_without_a_nonce_is_unusable() {
        assert!(parse_challenge(r#"Digest realm="x""#).is_none());
    }

    #[test]
    fn quoted_commas_do_not_split_a_value() {
        let c = parse_challenge(r#"Digest realm="a,b", nonce="n", qop="auth,auth-int""#).unwrap();
        assert_eq!(c.realm, "a,b");
        assert_eq!(c.qop.as_deref(), Some("auth,auth-int"));
        // auth is on offer among the list, so it is answerable.
        assert!(c.is_supported());
        assert_eq!(c.chosen_qop(), Some("auth"));
    }

    #[test]
    fn unsupported_algorithms_and_qop_are_rejected() {
        let sha = Challenge {
            algorithm: Some("SHA-256".to_string()),
            nonce: "n".to_string(),
            ..Default::default()
        };
        assert!(!sha.is_supported());

        let int_only = Challenge {
            qop: Some("auth-int".to_string()),
            nonce: "n".to_string(),
            ..Default::default()
        };
        assert!(!int_only.is_supported());

        let md5_sess = Challenge {
            algorithm: Some("MD5-sess".to_string()),
            nonce: "n".to_string(),
            ..Default::default()
        };
        assert!(md5_sess.is_supported());
    }

    /// RFC 7616 §3.9.1's challenge format: base64-ish nonces, an *unquoted*
    /// `algorithm=MD5`, and `qop="auth, auth-int"` with a space after the comma.
    ///
    /// The response asserted here is NOT the value printed in §3.9.1: that example is
    /// known to be inconsistent (Errata 4495 covers the password's capitalisation, and
    /// the printed digest matches neither casing). The value below was cross-checked
    /// against an independent MD5 implementation, and the canonical RFC 2617 vector in
    /// the next test is what actually pins the algorithm.
    #[test]
    fn handles_the_rfc7616_challenge_format() {
        let c = parse_challenge(
            r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#,
        )
        .unwrap();
        assert_eq!(c.realm, "http-auth@example.org");
        assert_eq!(c.algorithm.as_deref(), Some("MD5"));

        let header = authorization_header(
            &c,
            "Mufasa",
            "Circle of Life",
            "GET",
            "/dir/index.html",
            "f2/wE4q74E6zIJEtWaHKAf1J1RjkjRs4H4Ke3xtP7Fs=",
            1,
        );
        assert!(
            header.contains("response=\"e6c4e4db11fda5f3c4c16106316b674f\""),
            "{header}"
        );
        assert!(header.contains("qop=auth"), "{header}");
        assert!(header.contains("nc=00000001"), "{header}");
        assert!(header.contains("algorithm=MD5"), "{header}");
        assert!(
            header.contains("opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\""),
            "{header}"
        );
    }

    /// RFC 2617 §3.5 — the canonical, widely-reproduced vector. This is the test
    /// that actually pins HA1/HA2/response.
    #[test]
    fn matches_the_rfc2617_example() {
        let c = parse_challenge(
            r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#,
        )
        .unwrap();
        let header = authorization_header(
            &c,
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            "0a4f113b",
            1,
        );
        assert!(
            header.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "{header}"
        );
    }

    #[test]
    fn legacy_rfc2069_form_omits_qop_nc_and_cnonce() {
        let c = parse_challenge(r#"Digest realm="r", nonce="n""#).unwrap();
        let header = authorization_header(&c, "u", "p", "PROPFIND", "/dav/", "ignored", 1);
        assert!(!header.contains("qop="), "{header}");
        assert!(!header.contains("nc="), "{header}");
        assert!(!header.contains("cnonce="), "{header}");
        // HA1:nonce:HA2 with no qop in the middle.
        let ha1 = md5_hex("u:r:p");
        let ha2 = md5_hex("PROPFIND:/dav/");
        let expected = md5_hex(&format!("{ha1}:n:{ha2}"));
        assert!(
            header.contains(&format!("response=\"{expected}\"")),
            "{header}"
        );
    }

    #[test]
    fn md5_sess_folds_the_nonces_into_ha1() {
        let plain = Challenge {
            realm: "r".to_string(),
            nonce: "n".to_string(),
            qop: Some("auth".to_string()),
            algorithm: Some("MD5".to_string()),
            ..Default::default()
        };
        let sess = Challenge {
            algorithm: Some("MD5-sess".to_string()),
            ..plain.clone()
        };
        let a = authorization_header(&plain, "u", "p", "GET", "/", "cn", 1);
        let b = authorization_header(&sess, "u", "p", "GET", "/", "cn", 1);
        assert_ne!(a, b, "MD5-sess must derive a different response");
    }

    #[test]
    fn the_nonce_count_is_eight_hex_digits() {
        let c = parse_challenge(r#"Digest realm="r", nonce="n", qop="auth""#).unwrap();
        let header = authorization_header(&c, "u", "p", "GET", "/", "cn", 42);
        assert!(header.contains("nc=0000002a"), "{header}");
    }
}
