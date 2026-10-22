//! Sessions: how a browser signed in with Telegram shows it is the account's.
//!
//! A browser that has not signed in is known by the id it made, sent in
//! `x-tradoor-owner`, and knowing that id is what makes its things its own. An
//! account's id must not work that way: whoever saw it — in Settings over a
//! shoulder, in a screenshot — could act as the account for good, and signing
//! out could not take it back. So a signed-in browser holds a session instead,
//! sent in `x-tradoor-session`: a random token of its own, of which the server
//! keeps only a hash, and which signing out of that browser, or of all of them,
//! ends. A request that names an account's id itself is refused.
//!
//! Every handler still reads the owner from `x-tradoor-owner` (`api::owner`):
//! `middleware` writes the session's account there, over whatever was sent.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sha2::{Digest, Sha256};
use tradoor_store::now_ms;

use crate::api::{AppState, OWNER_HEADER};

/// The header a signed-in browser sends its session in.
pub const SESSION_HEADER: &str = "x-tradoor-session";

pub struct Sessions {
    pool: deadpool_postgres::Pool,
    /// The accounts' ids: only a session may act as one.
    accounts: RwLock<HashSet<String>>,
    /// Each live session's token hash, and the id of its account.
    live: RwLock<HashMap<String, String>>,
}

/// What is kept of a token: its SHA-256, in hex. A leaked table opens nothing.
fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn fresh_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system's randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Sessions {
    /// Every account and live session, as the database has them.
    pub async fn load(pool: deadpool_postgres::Pool) -> anyhow::Result<Arc<Self>> {
        let client = pool.get().await?;
        let accounts = client.query("SELECT owner FROM account", &[]).await?.iter().map(|r| r.get(0)).collect();
        let live = client.query("SELECT token_hash, owner FROM account_session", &[]).await?.iter().map(|r| (r.get(0), r.get(1))).collect();
        drop(client);
        Ok(Arc::new(Self { pool, accounts: RwLock::new(accounts), live: RwLock::new(live) }))
    }

    pub fn is_account(&self, owner: &str) -> bool {
        self.accounts.read().is_ok_and(|a| a.contains(owner))
    }

    /// The account a token is a live session of.
    pub fn owner_of(&self, token: &str) -> Option<String> {
        self.live.read().ok()?.get(&hash(token)).cloned()
    }

    /// An account made by a sign-in just confirmed: from now on its id alone is refused.
    pub fn add_account(&self, owner: &str) {
        if let Ok(mut a) = self.accounts.write() {
            a.insert(owner.to_string());
        }
    }

    /// A new session of the account, for the browser that signed in: its token,
    /// which is handed over this once and kept nowhere.
    pub async fn open(&self, owner: &str, telegram_user: i64, agent: Option<&str>) -> anyhow::Result<String> {
        let token = fresh_token();
        let key = hash(&token);
        let agent = agent.map(|a| a.chars().take(200).collect::<String>());
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO account_session (token_hash, owner, telegram_user, created_at, agent) VALUES ($1, $2, $3, $4, $5)",
                &[&key, &owner, &telegram_user, &now_ms(), &agent],
            )
            .await?;
        self.add_account(owner);
        if let Ok(mut live) = self.live.write() {
            live.insert(key, owner.to_string());
        }
        Ok(token)
    }

    /// Ends one session: signing out of a browser.
    pub async fn close(&self, token: &str) -> anyhow::Result<bool> {
        let key = hash(token);
        let n = self.pool.get().await?.execute("DELETE FROM account_session WHERE token_hash = $1", &[&key]).await?;
        if let Ok(mut live) = self.live.write() {
            live.remove(&key);
        }
        Ok(n > 0)
    }

    /// Ends every session of the account: signing out everywhere.
    pub async fn close_all(&self, owner: &str) -> anyhow::Result<u64> {
        let n = self.pool.get().await?.execute("DELETE FROM account_session WHERE owner = $1", &[&owner]).await?;
        if let Ok(mut live) = self.live.write() {
            live.retain(|_, o| o != owner);
        }
        Ok(n)
    }

    /// How many browsers are signed in to the account.
    pub fn count(&self, owner: &str) -> usize {
        self.live.read().map_or(0, |live| live.values().filter(|o| *o == owner).count())
    }
}

/// Whose request this is, once sessions are taken into account; a refusal
/// says to sign in again.
#[derive(Debug, PartialEq)]
enum Who {
    /// A session's account, written over what the browser sent.
    Account(String),
    /// As sent: a browser's own id, or nothing.
    AsSent,
    /// A session that has ended, or an account's id sent without one.
    SignedOut(&'static str),
}

fn who(sessions: &Sessions, headers: &HeaderMap) -> Who {
    if let Some(token) = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
        return match sessions.owner_of(token) {
            Some(owner) => Who::Account(owner),
            None => Who::SignedOut("this browser was signed out: sign in with Telegram again"),
        };
    }
    match headers.get(OWNER_HEADER).and_then(|v| v.to_str().ok()) {
        Some(id) if sessions.is_account(id) => Who::SignedOut("this id belongs to an account: sign in with Telegram to use it"),
        _ => Who::AsSent,
    }
}

/// Every API request passes here (see the module's note).
pub async fn middleware(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    match who(&state.sessions, req.headers()) {
        Who::Account(owner) => match HeaderValue::from_str(&owner) {
            Ok(v) => {
                req.headers_mut().insert(OWNER_HEADER, v);
            }
            Err(_) => return signed_out("this session's account cannot be read"),
        },
        Who::AsSent => {}
        Who::SignedOut(why) => return signed_out(why),
    }
    next.run(req).await
}

/// `401` with a flag the page acts on: it drops what it held and starts as a new browser.
fn signed_out(why: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": why, "signedOut": true }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessions(accounts: &[&str], live: &[(&str, &str)]) -> Sessions {
        let pool = deadpool_postgres::Config { url: Some("postgres://unused@127.0.0.1/unused".into()), ..Default::default() }
            .create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls)
            .unwrap();
        Sessions {
            pool,
            accounts: RwLock::new(accounts.iter().map(|a| a.to_string()).collect()),
            live: RwLock::new(live.iter().map(|(t, o)| (hash(t), o.to_string())).collect()),
        }
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn an_account_is_reached_through_a_session_and_never_by_its_id() {
        let s = sessions(&["account-id-0000001"], &[("token-a", "account-id-0000001")]);
        assert_eq!(who(&s, &headers(&[(SESSION_HEADER, "token-a")])), Who::Account("account-id-0000001".into()));
        assert_eq!(
            who(&s, &headers(&[(SESSION_HEADER, "token-a"), (OWNER_HEADER, "someone-else-00001")])),
            Who::Account("account-id-0000001".into()),
            "the session decides, whatever id came with it"
        );
        assert!(matches!(who(&s, &headers(&[(OWNER_HEADER, "account-id-0000001")])), Who::SignedOut(_)), "an account's id alone is refused");
        assert!(matches!(who(&s, &headers(&[(SESSION_HEADER, "token-gone")])), Who::SignedOut(_)), "an ended session is refused");
        assert_eq!(who(&s, &headers(&[(OWNER_HEADER, "a-browser-of-its-own")])), Who::AsSent, "a browser that never signed in is its own id");
        assert_eq!(who(&s, &HeaderMap::new()), Who::AsSent);
    }

    #[test]
    fn only_a_hash_of_a_token_is_kept() {
        let t = fresh_token();
        assert_eq!(t.len(), 64);
        assert_ne!(t, fresh_token());
        assert_eq!(hash(&t).len(), 64);
        assert_ne!(hash(&t), t);
        assert_eq!(hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
