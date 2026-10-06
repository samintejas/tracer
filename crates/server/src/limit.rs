//! A small in-memory rate limiter: a fixed window per key. One process is enough for one household server;
//! put a proxy-level limit in front for anything bigger.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pebblelab_core::Error;

pub struct Limiter {
    max: u32,
    window: Duration,
    hits: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Limiter {
    pub fn new(max: u32, window: Duration) -> Self {
        Limiter { max, window, hits: Mutex::new(HashMap::new()) }
    }

    /// Count one try for `key`. Errors when the key has used up its tries in this window.
    pub fn hit(&self, key: &str) -> Result<(), Error> {
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if hits.len() > 10_000 {
            let window = self.window;
            hits.retain(|_, (start, _)| now.duration_since(*start) < window);
        }
        let entry = hits.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > self.max {
            let wait = self.window.saturating_sub(now.duration_since(entry.0)).as_secs() / 60 + 1;
            return Err(Error::Limited(format!("too many tries: wait {wait} min and try again")));
        }
        Ok(())
    }
}

/// The limits the routes share.
pub struct Limits {
    /// Sign-in attempts for one address from one place.
    pub sign_in: Limiter,
    /// Sign-in attempts from one place, whatever the address.
    pub sign_in_ip: Limiter,
    pub sign_up: Limiter,
    /// Password reset emails, by place and by address, so nobody can use us to flood an inbox.
    pub reset: Limiter,
    /// Tries at an invite code by one person.
    pub join: Limiter,
    pub trust_proxy: bool,
}

impl Limits {
    pub fn new(trust_proxy: bool) -> Self {
        let quarter = Duration::from_secs(15 * 60);
        Limits {
            sign_in: Limiter::new(8, quarter),
            sign_in_ip: Limiter::new(60, quarter),
            sign_up: Limiter::new(10, Duration::from_secs(3600)),
            reset: Limiter::new(5, Duration::from_secs(3600)),
            join: Limiter::new(10, Duration::from_secs(3600)),
            trust_proxy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_after_the_limit_and_keys_are_separate() {
        let l = Limiter::new(2, Duration::from_secs(60));
        assert!(l.hit("a").is_ok());
        assert!(l.hit("a").is_ok());
        assert!(l.hit("a").is_err());
        assert!(l.hit("b").is_ok());
    }
}
