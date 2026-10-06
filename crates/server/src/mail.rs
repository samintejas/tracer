//! Email, sent through Resend (https://resend.com). With no API key nothing is sent and the server says so
//! in its log, so a household that does not want email can leave it off.

use serde_json::json;

pub struct Mailer {
    key: Option<String>,
    from: String,
    client: reqwest::Client,
}

impl Mailer {
    pub fn new(key: Option<String>, from: String) -> Self {
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap_or_default();
        Mailer { key: key.filter(|k| !k.trim().is_empty()), from, client }
    }

    /// A mailer that sends nothing.
    #[cfg(test)]
    pub fn off() -> Self {
        Mailer::new(None, String::new())
    }

    pub fn enabled(&self) -> bool {
        self.key.is_some()
    }

    pub async fn send(&self, to: &str, subject: &str, html: &str, text: &str) -> Result<(), String> {
        let Some(key) = &self.key else { return Err("email is not set up (TRACER_RESEND_API_KEY)".into()) };
        let res = self
            .client
            .post("https://api.resend.com/emails")
            .bearer_auth(key)
            .json(&json!({ "from": self.from, "to": [to], "subject": subject, "html": html, "text": text }))
            .send()
            .await
            .map_err(|e| format!("resend: {e}"))?;
        if res.status().is_success() {
            return Ok(());
        }
        let status = res.status();
        Err(format!("resend said {status}: {}", res.text().await.unwrap_or_default()))
    }

    /// The email with the link that sets a new password.
    pub async fn password_reset(&self, to: &str, name: &str, link: &str) -> Result<(), String> {
        let hi = name.split_whitespace().next().unwrap_or("there");
        let text = format!("hi {hi},\n\nsomeone asked to reset the password for your tracer/fin account. to choose a new one, open this link within an hour:\n\n{link}\n\nif it was not you, ignore this email: nothing changes.\n");
        let html = format!(
            "<div style=\"font-family:system-ui,sans-serif;max-width:480px;line-height:1.5;color:#1a1a1a\">\
             <p>hi {},</p>\
             <p>someone asked to reset the password for your tracer/fin account. to choose a new one, use the button within an hour.</p>\
             <p><a href=\"{link}\" style=\"display:inline-block;padding:10px 18px;background:#1a1a1a;color:#fff;border-radius:6px;text-decoration:none\">choose a new password</a></p>\
             <p style=\"color:#666;font-size:13px\">if it was not you, ignore this email: nothing changes.</p></div>",
            escape(hi)
        );
        self.send(to, "reset your tracer/fin password", &html, &text).await
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
