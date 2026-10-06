//! Talking to Google and GitHub. The rules for what a sign-in means are in `pebblelab-core`; this is only the
//! conversation with the provider: where to send the browser, and who the provider says came back.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use pebblelab_core::{Error, ExternalIdentity};

/// What differs between providers.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// OpenID Connect: one userinfo call says who it is and whether the email is verified.
    Oidc,
    /// Plain OAuth2: the profile and the emails are two calls.
    GitHub,
}

#[derive(Clone)]
pub struct Provider {
    pub kind: Kind,
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    /// GitHub only: where the person's emails are listed.
    pub emails_url: String,
    pub scope: &'static str,
}

impl Provider {
    pub fn google(client_id: String, client_secret: String) -> Self {
        Provider {
            kind: Kind::Oidc,
            client_id,
            client_secret,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo".into(),
            emails_url: String::new(),
            scope: "openid email profile",
        }
    }

    pub fn github(client_id: String, client_secret: String) -> Self {
        Provider {
            kind: Kind::GitHub,
            client_id,
            client_secret,
            auth_url: "https://github.com/login/oauth/authorize".into(),
            token_url: "https://github.com/login/oauth/access_token".into(),
            userinfo_url: "https://api.github.com/user".into(),
            emails_url: "https://api.github.com/user/emails".into(),
            scope: "read:user user:email",
        }
    }
}

/// The providers that are set up, and the address this server is reached at (the provider sends people back
/// to it).
pub struct Oauth {
    public_url: String,
    providers: HashMap<&'static str, Provider>,
    http: reqwest::Client,
    password: bool,
}

fn failed(provider: &str, what: &str) -> Error {
    tracing::warn!("sign-in with {provider}: {what}");
    Error::bad(format!("could not sign in with {provider}: try again"))
}

impl Oauth {
    pub fn new(public_url: &str, providers: HashMap<&'static str, Provider>) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("pebblelab-fin").build().expect("an http client");
        Oauth { public_url: public_url.trim_end_matches('/').to_string(), providers, http, password: true }
    }

    /// Whether people may also sign up and in with an email and a password (on unless turned off).
    pub fn with_password(mut self, on: bool) -> Self {
        self.password = on;
        self
    }

    pub fn password(&self) -> bool {
        self.password
    }

    /// Nothing set up: the sign-in buttons do not show.
    #[cfg(test)]
    pub fn none() -> Self {
        Oauth::new("", HashMap::new())
    }

    pub fn enabled(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.providers.keys().copied().collect();
        names.sort();
        names
    }

    /// Cookies for the sign-in dance are only sent over https when the server is reached over https.
    pub fn secure(&self) -> bool {
        self.public_url.starts_with("https://")
    }

    fn provider(&self, name: &str) -> Result<&Provider, Error> {
        self.providers.get(name).ok_or(Error::NotFound("provider"))
    }

    fn redirect_uri(&self, name: &str) -> String {
        format!("{}/api/auth/{name}/callback", self.public_url)
    }

    /// Where to send the browser to sign in. `verifier` is the PKCE secret; only its hash goes out.
    pub fn authorize_url(&self, name: &str, state: &str, verifier: &str) -> Result<String, Error> {
        let p = self.provider(name)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let redirect = self.redirect_uri(name);
        let mut q = vec![
            ("client_id", p.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", p.scope),
            ("state", state),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ];
        if p.kind == Kind::Oidc {
            q.push(("prompt", "select_account"));
        }
        let query = serde_urlencoded::to_string(&q).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(format!("{}?{query}", p.auth_url))
    }

    /// The provider sent the person back with `code`: ask it who they are. The code is traded for an access
    /// token straight with the provider over tls, which is what makes what it says about them believable.
    pub async fn identity(&self, name: &str, code: &str, verifier: &str) -> Result<ExternalIdentity, Error> {
        let p = self.provider(name)?;
        let redirect = self.redirect_uri(name);
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
            ("client_id", p.client_id.as_str()),
            ("client_secret", p.client_secret.as_str()),
            ("code_verifier", verifier),
        ];
        #[derive(Deserialize)]
        struct Token {
            access_token: Option<String>,
            error: Option<String>,
        }
        let token: Token = self
            .http
            .post(&p.token_url)
            .header("accept", "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|e| failed(name, &format!("token request: {e}")))?
            .json()
            .await
            .map_err(|e| failed(name, &format!("token reply: {e}")))?;
        let access = token.access_token.ok_or_else(|| failed(name, &format!("no token: {}", token.error.unwrap_or_default())))?;
        let get = |url: &str| self.http.get(url).bearer_auth(&access).header("accept", "application/json");
        match p.kind {
            Kind::Oidc => {
                #[derive(Deserialize)]
                struct Info {
                    sub: String,
                    #[serde(default)]
                    email: String,
                    #[serde(default)]
                    email_verified: bool,
                    #[serde(default)]
                    name: String,
                    picture: Option<String>,
                }
                let i: Info = get(&p.userinfo_url).send().await.map_err(|e| failed(name, &format!("userinfo: {e}")))?.json().await.map_err(|e| failed(name, &format!("userinfo reply: {e}")))?;
                let picture = self.fetch_picture(i.picture.as_deref(), &["googleusercontent.com"]).await;
                Ok(ExternalIdentity { provider: name.into(), subject: i.sub, email: i.email, email_verified: i.email_verified, name: i.name, picture })
            }
            Kind::GitHub => {
                #[derive(Deserialize)]
                struct Profile {
                    id: i64,
                    #[serde(default)]
                    login: String,
                    name: Option<String>,
                    avatar_url: Option<String>,
                }
                #[derive(Deserialize)]
                struct Mail {
                    email: String,
                    #[serde(default)]
                    primary: bool,
                    #[serde(default)]
                    verified: bool,
                }
                let who: Profile = get(&p.userinfo_url).send().await.map_err(|e| failed(name, &format!("profile: {e}")))?.json().await.map_err(|e| failed(name, &format!("profile reply: {e}")))?;
                let mails: Vec<Mail> = get(&p.emails_url).send().await.map_err(|e| failed(name, &format!("emails: {e}")))?.json().await.map_err(|e| failed(name, &format!("emails reply: {e}")))?;
                // the verified primary address; failing that, say what it is and that nobody vouched for it
                let pick = mails.iter().find(|m| m.primary && m.verified).or_else(|| mails.iter().find(|m| m.verified)).or_else(|| mails.iter().find(|m| m.primary));
                let (email, verified) = pick.map(|m| (m.email.clone(), m.verified)).unwrap_or_default();
                let avatar = who.avatar_url.as_deref().map(|u| format!("{u}{}s=160", if u.contains('?') { '&' } else { '?' }));
                let picture = self.fetch_picture(avatar.as_deref(), &["githubusercontent.com"]).await;
                Ok(ExternalIdentity { provider: name.into(), subject: who.id.to_string(), email, email_verified: verified, name: who.name.filter(|n| !n.trim().is_empty()).unwrap_or(who.login), picture })
            }
        }
    }
}

impl Oauth {
    /// Download a provider's profile picture and turn it into a `data:` url, since the app's CSP only shows
    /// images it serves itself. Only https hosts on `hosts` are fetched, and only small raster images are
    /// kept. Never an error: a person without a picture signs in just the same.
    async fn fetch_picture(&self, url: Option<&str>, hosts: &[&str]) -> Option<String> {
        use base64::Engine;
        let url = reqwest::Url::parse(url?).ok()?;
        let host = url.host_str()?;
        if url.scheme() != "https" || !hosts.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
            return None;
        }
        let res = self.http.get(url).send().await.ok()?.error_for_status().ok()?;
        let mime = res.headers().get("content-type")?.to_str().ok()?.split(';').next()?.trim().to_lowercase();
        if !["image/jpeg", "image/png", "image/webp", "image/gif"].contains(&mime.as_str()) {
            return None;
        }
        let bytes = res.bytes().await.ok()?;
        if bytes.is_empty() || bytes.len() > 250_000 {
            return None;
        }
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
    }
}
