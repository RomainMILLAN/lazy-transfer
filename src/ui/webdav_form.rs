use crate::transfer::types::{parse_webdav_url, ConnectionConfig, WebDavAuth};

/// What the UI should do next. `App` maps these onto its `InputMode` / dialogs; all
/// the decisions live here so the flow is unit-testable without a terminal.
#[derive(Debug)]
pub enum WebDavStep {
    AskUser,
    AskPassword,
    AskToken,
    /// Boxed to keep the enum small — `ConnectionConfig` is by far the biggest variant.
    Connect(Box<ConnectionConfig>),
    /// The value could not be used; the message is meant for `info_msg`.
    Invalid(String),
}

/// The WebDAV connection form.
///
/// `auth` carries user + secret + KIND together, which is exactly `WebDavAuth`.
/// Keeping the VO rather than a `user` + `secret` pair is what makes editing a
/// saved Bearer connection safe: with separate fields the token would silently be
/// reused as a Basic password.
pub struct WebDavForm {
    /// Normalized once validated; on `prefilled` it is whatever was persisted.
    url: String,
    auth: WebDavAuth,
    insecure_tls: bool,
}

impl Default for WebDavForm {
    fn default() -> Self {
        Self::new()
    }
}

impl WebDavForm {
    pub fn new() -> Self {
        WebDavForm {
            url: String::new(),
            auth: WebDavAuth::Anonymous,
            insecure_tls: false,
        }
    }

    /// Seeds the form when editing a saved connection (the `e` key). Without this
    /// the edit path could not pre-fill anything and `insecure_tls` would be dead.
    pub fn prefilled(url: String, auth: WebDavAuth, insecure_tls: bool) -> Self {
        WebDavForm {
            url,
            auth,
            insecure_tls,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Pre-fill value for the user prompt (empty when unknown).
    pub fn user(&self) -> &str {
        self.auth.user().unwrap_or("")
    }

    /// Drives the "leave empty to keep current password" hint.
    pub fn has_secret(&self) -> bool {
        self.auth.secret().is_some()
    }

    /// The auth kind carried over from a saved connection, for defaulting the
    /// choice dialog. `None` on a fresh form.
    pub fn known_auth_kind(&self) -> Option<char> {
        match self.auth {
            WebDavAuth::Basic { .. } => Some('b'),
            WebDavAuth::Digest { .. } => Some('d'),
            WebDavAuth::Bearer(_) => Some('t'),
            WebDavAuth::Anonymous => None,
        }
    }

    pub fn insecure_tls(&self) -> bool {
        self.insecure_tls
    }

    pub fn set_insecure_tls(&mut self, insecure: bool) {
        self.insecure_tls = insecure;
    }

    /// Validates and normalizes the URL. `Err` keeps the caller on the same field.
    pub fn submit_url(&mut self, value: String) -> Result<(), String> {
        let parsed = parse_webdav_url(&value)?;
        // A username pasted in the URL pre-fills Basic auth; the password half was
        // already dropped by the parser.
        if let Some(user) = parsed.user.clone() {
            if self.auth.user().is_none() {
                self.auth = WebDavAuth::Basic {
                    user,
                    password: String::new(),
                };
            }
        }
        self.url = parsed.normalized;
        Ok(())
    }

    pub fn choose_auth(&mut self, key: char) -> WebDavStep {
        match key {
            'b' | 'd' => {
                // Reuse a carried-over user/password pair across Basic<->Digest, but
                // never a bearer token: that would silently send the token as a
                // password to a different scheme.
                let (user, password) = match &self.auth {
                    WebDavAuth::Basic { user, password }
                    | WebDavAuth::Digest { user, password } => (user.clone(), password.clone()),
                    _ => (String::new(), String::new()),
                };
                self.auth = if key == 'd' {
                    WebDavAuth::Digest { user, password }
                } else {
                    WebDavAuth::Basic { user, password }
                };
                WebDavStep::AskUser
            }
            't' => {
                let token = match &self.auth {
                    WebDavAuth::Bearer(t) => t.clone(),
                    _ => String::new(),
                };
                self.auth = WebDavAuth::Bearer(token);
                WebDavStep::AskToken
            }
            'n' => {
                self.auth = WebDavAuth::Anonymous;
                self.build()
            }
            other => WebDavStep::Invalid(format!("unknown auth choice '{other}'")),
        }
    }

    pub fn submit_user(&mut self, value: String) -> WebDavStep {
        let (password, is_digest) = match &self.auth {
            WebDavAuth::Basic { password, .. } => (password.clone(), false),
            WebDavAuth::Digest { password, .. } => (password.clone(), true),
            _ => (String::new(), false),
        };
        self.auth = if is_digest {
            WebDavAuth::Digest {
                user: value,
                password,
            }
        } else {
            WebDavAuth::Basic {
                user: value,
                password,
            }
        };
        WebDavStep::AskPassword
    }

    /// An empty value keeps the carried-over password (edit flow).
    pub fn submit_password(&mut self, value: String) -> WebDavStep {
        let user = self.auth.user().unwrap_or("").to_string();
        let password = if value.is_empty() {
            self.auth.secret().unwrap_or("").to_string()
        } else {
            value
        };
        self.auth = if matches!(self.auth, WebDavAuth::Digest { .. }) {
            WebDavAuth::Digest { user, password }
        } else {
            WebDavAuth::Basic { user, password }
        };
        self.build()
    }

    /// An empty value keeps the carried-over token (edit flow).
    pub fn submit_token(&mut self, value: String) -> WebDavStep {
        let token = if value.is_empty() {
            self.auth.secret().unwrap_or("").to_string()
        } else {
            value
        };
        self.auth = WebDavAuth::Bearer(token);
        self.build()
    }

    /// Re-parses the stored URL with the same, idempotent parser. On the edit path
    /// the stored value comes from connections.json, which is hand-editable, so
    /// this is the moment an invalid URL is caught.
    fn build(&self) -> WebDavStep {
        match parse_webdav_url(&self.url) {
            Ok(parsed) => WebDavStep::Connect(Box::new(ConnectionConfig::webdav(
                &parsed,
                self.auth.clone(),
                self.insecure_tls,
            ))),
            Err(e) => WebDavStep::Invalid(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::types::Protocol;

    const URL: &str = "https://cloud.exemple.fr/remote.php/dav/files/romain/";

    fn connected(step: WebDavStep) -> ConnectionConfig {
        match step {
            WebDavStep::Connect(c) => *c,
            other => panic!("expected Connect, got {other:?}"),
        }
    }

    #[test]
    fn invalid_url_keeps_the_user_on_the_field() {
        let mut form = WebDavForm::new();
        assert!(form.submit_url("ftp://host/pub".to_string()).is_err());
        assert!(form.url().is_empty());
    }

    #[test]
    fn submit_url_normalizes() {
        let mut form = WebDavForm::new();
        form.submit_url("cloud.exemple.fr/dav".to_string()).unwrap();
        assert_eq!(form.url(), "https://cloud.exemple.fr/dav/");
    }

    #[test]
    fn basic_flow() {
        let mut form = WebDavForm::new();
        form.submit_url(URL.to_string()).unwrap();
        assert!(matches!(form.choose_auth('b'), WebDavStep::AskUser));
        assert!(matches!(
            form.submit_user("romain".to_string()),
            WebDavStep::AskPassword
        ));
        let conn = connected(form.submit_password("pw".to_string()));
        assert_eq!(conn.protocol(), &Protocol::WebDav);
        assert_eq!(
            conn.webdav_config().unwrap().auth,
            WebDavAuth::Basic {
                user: "romain".to_string(),
                password: "pw".to_string()
            }
        );
    }

    #[test]
    fn digest_flow() {
        let mut form = WebDavForm::new();
        form.submit_url(URL.to_string()).unwrap();
        assert!(matches!(form.choose_auth('d'), WebDavStep::AskUser));
        assert!(matches!(
            form.submit_user("romain".to_string()),
            WebDavStep::AskPassword
        ));
        let conn = connected(form.submit_password("pw".to_string()));
        assert_eq!(
            conn.webdav_config().unwrap().auth,
            WebDavAuth::Digest {
                user: "romain".to_string(),
                password: "pw".to_string()
            }
        );
    }

    #[test]
    fn editing_a_digest_connection_keeps_the_scheme_and_secret() {
        let mut form = WebDavForm::prefilled(
            URL.to_string(),
            WebDavAuth::Digest {
                user: "romain".to_string(),
                password: "old".to_string(),
            },
            false,
        );
        assert_eq!(form.known_auth_kind(), Some('d'));
        form.choose_auth('d');
        form.submit_user("romain".to_string());
        let conn = connected(form.submit_password(String::new()));
        assert_eq!(
            conn.webdav_config().unwrap().auth,
            WebDavAuth::Digest {
                user: "romain".to_string(),
                password: "old".to_string()
            }
        );
    }

    #[test]
    fn bearer_token_is_never_reused_as_a_digest_password() {
        let mut form = WebDavForm::prefilled(
            URL.to_string(),
            WebDavAuth::Bearer("secret-token".to_string()),
            false,
        );
        form.choose_auth('d');
        form.submit_user("romain".to_string());
        let conn = connected(form.submit_password(String::new()));
        let secret = conn.webdav_config().unwrap().auth.secret().unwrap_or("");
        assert_ne!(
            secret, "secret-token",
            "bearer token leaked into Digest auth"
        );
        assert!(secret.is_empty());
    }

    #[test]
    fn bearer_flow() {
        let mut form = WebDavForm::new();
        form.submit_url(URL.to_string()).unwrap();
        assert!(matches!(form.choose_auth('t'), WebDavStep::AskToken));
        let conn = connected(form.submit_token("tok".to_string()));
        assert_eq!(
            conn.webdav_config().unwrap().auth,
            WebDavAuth::Bearer("tok".to_string())
        );
    }

    #[test]
    fn anonymous_connects_immediately() {
        let mut form = WebDavForm::new();
        form.submit_url(URL.to_string()).unwrap();
        let conn = connected(form.choose_auth('n'));
        assert_eq!(conn.webdav_config().unwrap().auth, WebDavAuth::Anonymous);
    }

    #[test]
    fn unknown_auth_choice_is_reported_not_panicked() {
        let mut form = WebDavForm::new();
        form.submit_url(URL.to_string()).unwrap();
        assert!(matches!(form.choose_auth('z'), WebDavStep::Invalid(_)));
    }

    #[test]
    fn url_userinfo_prefills_the_user() {
        let mut form = WebDavForm::new();
        form.submit_url("https://alice@cloud/dav/".to_string())
            .unwrap();
        assert_eq!(form.user(), "alice");
        assert!(!form.url().contains("alice"));
    }

    #[test]
    fn prefilled_keeps_insecure_tls_through_to_connect() {
        let mut form = WebDavForm::prefilled(
            URL.to_string(),
            WebDavAuth::Basic {
                user: "romain".to_string(),
                password: "old".to_string(),
            },
            true,
        );
        assert!(form.insecure_tls());
        let conn = connected(form.choose_auth('n'));
        assert!(conn.webdav_config().unwrap().insecure_tls);
    }

    #[test]
    fn empty_password_on_edit_keeps_the_old_secret() {
        let mut form = WebDavForm::prefilled(
            URL.to_string(),
            WebDavAuth::Basic {
                user: "romain".to_string(),
                password: "old".to_string(),
            },
            false,
        );
        assert!(form.has_secret());
        form.choose_auth('b');
        form.submit_user("romain".to_string());
        let conn = connected(form.submit_password(String::new()));
        assert_eq!(
            conn.webdav_config().unwrap().auth.secret(),
            Some("old"),
            "an empty password must keep the current one"
        );
    }

    #[test]
    fn empty_token_on_edit_keeps_the_old_token() {
        let mut form =
            WebDavForm::prefilled(URL.to_string(), WebDavAuth::Bearer("old".into()), false);
        form.choose_auth('t');
        let conn = connected(form.submit_token(String::new()));
        assert_eq!(conn.webdav_config().unwrap().auth.secret(), Some("old"));
    }

    /// The bug this design exists to prevent: editing a Bearer connection and
    /// picking Basic must NOT reuse the token as a password.
    #[test]
    fn bearer_token_is_never_reused_as_a_basic_password() {
        let mut form = WebDavForm::prefilled(
            URL.to_string(),
            WebDavAuth::Bearer("secret-token".to_string()),
            false,
        );
        assert_eq!(form.known_auth_kind(), Some('t'));
        form.choose_auth('b');
        form.submit_user("romain".to_string());
        // Empty password with no Basic password carried over: must stay empty,
        // never fall back to the bearer token.
        let conn = connected(form.submit_password(String::new()));
        let secret = conn.webdav_config().unwrap().auth.secret().unwrap_or("");
        assert_ne!(
            secret, "secret-token",
            "bearer token leaked into Basic auth"
        );
        assert!(secret.is_empty());
    }

    #[test]
    fn build_reports_a_hand_edited_invalid_url() {
        // connections.json is hand-editable: the edit flow is where that surfaces.
        let mut form = WebDavForm::prefilled("not a url".to_string(), WebDavAuth::Anonymous, false);
        assert!(matches!(form.choose_auth('n'), WebDavStep::Invalid(_)));
    }
}
