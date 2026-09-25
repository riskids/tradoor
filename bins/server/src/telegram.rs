//! Alerts on Telegram: linking a chat to a browser id, and the bot that carries
//! the messages.
//!
//! A bot cannot open a conversation, so the reader opens it: Settings follows
//! `t.me/<bot>?start=<code>` with a one-use code minted here, Telegram sends the
//! bot `/start <code>`, and the code says which browser the chat belongs to. No
//! phone number is asked for and no chat id is typed anywhere.
//!
//! Updates arrive by long polling — one request the server holds open — so
//! nothing has to be reachable from the internet for this to work, on a VPS or
//! on a laptop. The token is read from the environment and never logged.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tradoor_core::format::{format_price, price_precision};
use tradoor_core::time::iso_minute;
use tradoor_store::now_ms;
use tradoor_venues::MarketRef;

use crate::api::{owner, ApiError, ApiResult, AppState};
use crate::dns::Doh;

/// How long a link code is good for: long enough to walk to the phone.
const CODE_GOOD_FOR: i64 = 15 * 60_000;
const CODE_LEN: usize = 16;
/// Telegram allows about one message a second to a chat; this stays under it.
const SEND_GAP: Duration = Duration::from_millis(250);
/// Seconds Telegram holds a poll open with nothing to say.
const POLL_SECS: u64 = 25;
/// Messages waiting to go out. Past this the oldest are dropped rather than
/// holding up an alert: a notification late by minutes is worse than missing.
const QUEUE: usize = 256;
/// Telegram lets a bot take back its own message for 48 hours, so a window must
/// leave room to sweep before that runs out.
pub const MAX_CLEAR_HOURS: i32 = 47;
/// How often messages whose time is up are taken back.
const SWEEP: Duration = Duration::from_secs(60);
const MAX_TEXT: usize = 3500;

pub struct Chat {
    pub chat_id: i64,
    pub name: Option<String>,
    pub linked_at: i64,
    /// Minutes east of UTC, as the reader's own settings write times.
    pub utc_offset: i32,
    pub zone: Option<String>,
    /// Hours a message stays before the bot takes it back; none to keep them.
    pub clear_after_hours: Option<i32>,
}


pub struct Telegram {
    http: reqwest::Client,
    token: String,
    pool: deadpool_postgres::Pool,
    /// The bot's @name, for the link Settings opens. Read once it answers.
    bot: RwLock<Option<String>>,
    out: mpsc::Sender<(i64, String, Option<i32>)>,
}

impl Telegram {
    pub fn start(pool: deadpool_postgres::Pool, token: String, doh: &Arc<Doh>) -> anyhow::Result<Arc<Self>> {
        let http = reqwest::Client::builder().dns_resolver(doh.resolver()).timeout(Duration::from_secs(POLL_SECS + 15)).build()?;
        let (out, rx) = mpsc::channel(QUEUE);
        let telegram = Arc::new(Self { http, token, pool, bot: RwLock::new(None), out });
        tokio::spawn(send_queued(telegram.clone(), rx));
        tokio::spawn(sweep(telegram.clone()));
        tokio::spawn(poll(telegram.clone()));
        tracing::info!("telegram bot started");
        Ok(telegram)
    }

    pub fn bot(&self) -> Option<String> {
        self.bot.read().ok().and_then(|b| b.clone())
    }

    /// One Telegram method. The token is in the path, so an error from here is
    /// logged without the URL.
    async fn call(&self, method: &str, body: Value) -> anyhow::Result<Value> {
        let url = format!("https://api.telegram.org/bot{}/{method}", self.token);
        let res = self.http.post(url).header("content-type", "application/json").body(body.to_string()).send().await.map_err(|e| anyhow::anyhow!("{method}: {}", strip_token(&e.to_string(), &self.token)))?;
        let body: Value = serde_json::from_slice(&res.bytes().await?)?;
        if body["ok"] != true {
            anyhow::bail!("{method}: {}", body["description"].as_str().unwrap_or("Telegram refused it"));
        }
        Ok(body["result"].clone())
    }

    /// A code for this browser, and the link that hands it to the bot.
    pub async fn link(&self, owner: &str, offset: i32, zone: Option<&str>) -> ApiResult<(String, String)> {
        let bot = self.bot().ok_or_else(|| ApiError(axum::http::StatusCode::SERVICE_UNAVAILABLE, "the bot has not answered yet — try again in a moment".into()))?;
        let code = mint_code(&self.pool, owner, offset, zone).await?;
        Ok((code.clone(), format!("https://t.me/{bot}?start={code}")))
    }

    /// The reader moved, or chose another zone: later messages follow it.
    pub async fn set_clock(&self, owner: &str, offset: i32, zone: Option<&str>) -> ApiResult<u64> {
        let client = self.pool.get().await?;
        Ok(client.execute("UPDATE telegram_chat SET utc_offset = $2, zone = $3 WHERE owner = $1", &[&owner, &offset, &zone]).await?)
    }

    /// How long a message stays in the chat before the bot takes it back.
    pub async fn set_clearing(&self, owner: &str, hours: Option<i32>) -> ApiResult<u64> {
        let hours = hours.map(|h| h.clamp(1, MAX_CLEAR_HOURS));
        let client = self.pool.get().await?;
        Ok(client.execute("UPDATE telegram_chat SET clear_after_hours = $2 WHERE owner = $1", &[&owner, &hours]).await?)
    }

    pub async fn chats(&self, owner: &str) -> ApiResult<Vec<Chat>> {
        let client = self.pool.get().await?;
        let rows = client.query("SELECT chat_id, name, linked_at, utc_offset, zone, clear_after_hours FROM telegram_chat WHERE owner = $1 ORDER BY linked_at", &[&owner]).await?;
        Ok(rows.iter().map(|r| Chat { chat_id: r.get(0), name: r.get(1), linked_at: r.get(2), utc_offset: r.get(3), zone: r.get(4), clear_after_hours: r.get(5) }).collect())
    }

    pub async fn unlink(&self, owner: &str, chat_id: i64) -> ApiResult<bool> {
        let client = self.pool.get().await?;
        Ok(client.execute("DELETE FROM telegram_chat WHERE owner = $1 AND chat_id = $2", &[&owner, &chat_id]).await? > 0)
    }

    /// Queue a message to every chat this browser has linked, written in that
    /// chat's own hours. Dropped when the queue is full: an alert is not worth
    /// stalling the watcher for.
    pub async fn send_to_owner(&self, owner: &str, text: impl Fn(i32) -> String) {
        let chats = match self.chats(owner).await {
            Ok(chats) => chats,
            Err(e) => {
                tracing::warn!(error = %e.1, "reading the chats to notify");
                return;
            }
        };
        for chat in chats {
            if self.out.try_send((chat.chat_id, text(chat.utc_offset), chat.clear_after_hours)).is_err() {
                tracing::warn!(chat = chat.chat_id, "telegram queue full, message dropped");
            }
        }
    }
}

/// A code this browser can spend once, and a sweep of the stale ones.
async fn mint_code(pool: &deadpool_postgres::Pool, owner: &str, offset: i32, zone: Option<&str>) -> ApiResult<String> {
    let code = fresh_code();
    let client = pool.get().await?;
    client.execute("DELETE FROM telegram_link WHERE created_at < $1", &[&(now_ms() - CODE_GOOD_FOR)]).await?;
    client.execute("INSERT INTO telegram_link (code, owner, created_at, utc_offset, zone) VALUES ($1, $2, $3, $4, $5)", &[&code, &owner, &now_ms(), &offset, &zone]).await?;
    Ok(code)
}

/// Never let the token reach a log line, whatever an error says.
fn strip_token(message: &str, token: &str) -> String {
    message.replace(token, "…")
}

fn fresh_code() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyz23456789";
    let mut bytes = [0u8; CODE_LEN];
    getrandom::fill(&mut bytes).expect("the system's randomness");
    bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

/// One message at a time, spaced out: Telegram counts messages per chat and
/// answers a burst with `429`.
async fn send_queued(telegram: Arc<Telegram>, mut rx: mpsc::Receiver<(i64, String, Option<i32>)>) {
    while let Some((chat_id, text, clear_after)) = rx.recv().await {
        let body = json!({ "chat_id": chat_id, "text": text, "disable_web_page_preview": true });
        match telegram.call("sendMessage", body).await {
            // Only a message the bot can still name can be taken back later.
            Ok(sent) => {
                if let (Some(hours), Some(message_id)) = (clear_after, sent["message_id"].as_i64()) {
                    let clear_at = now_ms() + i64::from(hours.clamp(1, MAX_CLEAR_HOURS)) * 3_600_000;
                    if let Err(e) = remember(&telegram.pool, chat_id, message_id, clear_at).await {
                        tracing::warn!(chat = chat_id, error = %e, "keeping a message to clear");
                    }
                }
            }
            Err(e) => tracing::warn!(chat = chat_id, error = %e, "sending a telegram message"),
        }
        sleep(SEND_GAP).await;
    }
}

async fn remember(pool: &deadpool_postgres::Pool, chat_id: i64, message_id: i64, clear_at: i64) -> anyhow::Result<()> {
    let client = pool.get().await?;
    client
        .execute(
            "INSERT INTO telegram_message (chat_id, message_id, clear_at) VALUES ($1, $2, $3) ON CONFLICT (chat_id, message_id) DO NOTHING",
            &[&chat_id, &message_id, &clear_at],
        )
        .await?;
    Ok(())
}

/// Messages whose time is up, taken back so a chat does not fill with old
/// alerts. Telegram refuses one older than 48 hours, so the row goes either
/// way — a message left behind is only a message, and retrying it forever
/// would be noise in the log.
async fn sweep(telegram: Arc<Telegram>) {
    loop {
        sleep(SWEEP).await;
        let due = match take_due(&telegram.pool).await {
            Ok(due) => due,
            Err(e) => {
                tracing::warn!(error = %e, "reading the messages to clear");
                continue;
            }
        };
        for (chat_id, message_id) in due {
            if let Err(e) = telegram.call("deleteMessage", json!({ "chat_id": chat_id, "message_id": message_id })).await {
                tracing::debug!(chat = chat_id, error = %e, "clearing a telegram message");
            }
            sleep(SEND_GAP).await;
        }
    }
}

async fn take_due(pool: &deadpool_postgres::Pool) -> anyhow::Result<Vec<(i64, i64)>> {
    let client = pool.get().await?;
    let rows = client.query("DELETE FROM telegram_message WHERE clear_at <= $1 RETURNING chat_id, message_id", &[&now_ms()]).await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// What one update carries, of the little this bot reads.
#[derive(Debug, PartialEq)]
pub struct Incoming {
    pub id: i64,
    pub chat_id: i64,
    pub name: Option<String>,
    pub text: String,
}

pub fn read_updates(result: &Value) -> Vec<Incoming> {
    result
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|u| {
                    let message = u.get("message").or_else(|| u.get("edited_message"))?;
                    let chat = message.get("chat")?;
                    let name = chat.get("username").or_else(|| chat.get("first_name")).and_then(Value::as_str).map(str::to_string);
                    Some(Incoming { id: u["update_id"].as_i64()?, chat_id: chat["id"].as_i64()?, name, text: message["text"].as_str().unwrap_or_default().to_string() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The code in `/start <code>`, however the client spells the command.
pub fn start_code(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("/start")?;
    // A client in a group spells it `/start@thebot code`.
    let rest = rest.strip_prefix('@').map_or(rest, |mention| mention.split_once(char::is_whitespace).map_or("", |(_, code)| code));
    Some(rest.trim()).filter(|c| !c.is_empty() && c.len() <= 64)
}

async fn poll(telegram: Arc<Telegram>) {
    let mut offset: i64 = 0;
    let mut quiet = false;
    loop {
        if telegram.bot().is_none() {
            match telegram.call("getMe", json!({})).await {
                Ok(me) => {
                    let name = me["username"].as_str().unwrap_or_default().to_string();
                    tracing::info!(bot = %name, "telegram bot connected");
                    if let Ok(mut bot) = telegram.bot.write() {
                        *bot = Some(name);
                    }
                }
                Err(e) => {
                    if !quiet {
                        tracing::warn!(error = %e, "telegram bot did not answer");
                        quiet = true;
                    }
                    sleep(Duration::from_secs(30)).await;
                    continue;
                }
            }
        }
        match telegram.call("getUpdates", json!({ "offset": offset, "timeout": POLL_SECS, "allowed_updates": ["message"] })).await {
            Ok(result) => {
                quiet = false;
                for update in read_updates(&result) {
                    offset = update.id + 1;
                    let reply = handle(&telegram, &update).await;
                    let _ = telegram.out.try_send((update.chat_id, reply, None));
                }
            }
            Err(e) => {
                if !quiet {
                    tracing::warn!(error = %e, "polling telegram");
                    quiet = true;
                }
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// What the bot says back. The only thing it takes is a link code.
async fn handle(telegram: &Telegram, update: &Incoming) -> String {
    if update.text.starts_with("/stop") {
        return match forget(&telegram.pool, update.chat_id).await {
            Ok(0) => "This chat was not receiving alerts.".to_string(),
            Ok(_) => "Stopped. This chat will not receive alerts again until you link it from tradoor.".to_string(),
            Err(e) => {
                tracing::warn!(error = %e, "unlinking a chat");
                "Something went wrong here; try again in a moment.".to_string()
            }
        };
    }
    let Some(code) = start_code(&update.text) else {
        return "Open Settings in tradoor and press Connect Telegram; the link there brings the code that pairs us.".to_string();
    };
    match claim(&telegram.pool, code, update).await {
        Ok(true) => "Linked. Your tradoor alerts will arrive here. Send /stop to end it.".to_string(),
        Ok(false) => "That link has expired or was already used. Press Connect Telegram in tradoor again.".to_string(),
        Err(e) => {
            tracing::warn!(error = %e, "linking a chat");
            "Something went wrong here; try the link again in a moment.".to_string()
        }
    }
}

/// Spend the code and remember the chat. A code works once.
async fn claim(pool: &deadpool_postgres::Pool, code: &str, update: &Incoming) -> anyhow::Result<bool> {
    let client = pool.get().await?;
    let row = client.query_opt("DELETE FROM telegram_link WHERE code = $1 AND created_at > $2 RETURNING owner, utc_offset, zone", &[&code, &(now_ms() - CODE_GOOD_FOR)]).await?;
    let Some(row) = row else { return Ok(false) };
    let owner: String = row.get(0);
    let (offset, zone): (i32, Option<String>) = (row.get(1), row.get(2));
    client
        .execute(
            "INSERT INTO telegram_chat (owner, chat_id, name, linked_at, utc_offset, zone) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (owner, chat_id) DO UPDATE SET name = EXCLUDED.name, utc_offset = EXCLUDED.utc_offset, zone = EXCLUDED.zone",
            &[&owner, &update.chat_id, &update.name, &now_ms(), &offset, &zone],
        )
        .await?;
    tracing::info!(chat = update.chat_id, "telegram chat linked");
    Ok(true)
}

async fn forget(pool: &deadpool_postgres::Pool, chat_id: i64) -> anyhow::Result<u64> {
    let client = pool.get().await?;
    Ok(client.execute("DELETE FROM telegram_chat WHERE chat_id = $1", &[&chat_id]).await?)
}

/// Offsets outside this are not a clock's, whatever the page said.
const MAX_OFFSET: i32 = 14 * 60;

/// Minutes east of UTC as the page writes them: `UTC`, `UTC+8`, `UTC-3:30`.
pub fn offset_label(minutes: i32) -> String {
    if minutes == 0 {
        return "UTC".to_string();
    }
    let (sign, m) = if minutes < 0 { ('-', -minutes) } else { ('+', minutes) };
    match (m / 60, m % 60) {
        (h, 0) => format!("UTC{sign}{h}"),
        (h, rest) => format!("UTC{sign}{h}:{rest:02}"),
    }
}

/// A time in the reader's own hours, named so there is no guessing which.
pub fn stamp(at: i64, offset: i32) -> String {
    let offset = offset.clamp(-MAX_OFFSET, MAX_OFFSET);
    format!("{} {}", iso_minute(at + i64::from(offset) * 60_000), offset_label(offset))
}

/// What an alert says when it fires, in the page's own wording and hours. The
/// chart at the front says which way the price came to the level, since a chat
/// message carries no colour of its own.
pub fn alert_text(market: &str, condition: &str, level: f64, value: f64, start: Option<f64>, message: Option<&str>, at: i64, offset: i32) -> String {
    let digits = Some(price_precision([level, value]));
    let way = match crate::alerts::rose(condition, level, start) {
        Some(true) => "📈 ",
        Some(false) => "📉 ",
        None => "",
    };
    let mut text = format!("{way}{} {} {}\nfired at {} — {}", tv_name(market), condition_label(condition), format_price(level, digits), format_price(value, digits), stamp(at, offset));
    if let Some(note) = message.map(str::trim).filter(|m| !m.is_empty()) {
        text.push('\n');
        text.push_str(note);
    }
    text.truncate(MAX_TEXT);
    text
}

/// A market as TradingView names it: `BTCUSDT.P` for a perpetual.
fn tv_name(market: &str) -> String {
    match MarketRef::parse(market) {
        Some(m) if m.venue.id().ends_with("perp") => format!("{}.P", m.symbol),
        Some(m) => m.symbol,
        None => market.to_string(),
    }
}

fn condition_label(condition: &str) -> &str {
    match condition {
        "crossing" => "Crossing",
        "crossing_up" => "Crossing Up",
        "crossing_down" => "Crossing Down",
        "greater" => "Greater Than",
        "less" => "Less Than",
        other => other,
    }
}

/// Tell the reader an alert fired, if they linked a chat for it.
pub async fn notify_fired(telegram: Option<&Arc<Telegram>>, owner: &str, alert: &tokio_postgres::Row, value: f64, at: i64) {
    let Some(telegram) = telegram else { return };
    let (market, condition): (String, String) = (alert.get(1), alert.get(2));
    let (level, note, start): (f64, Option<String>, Option<f64>) = (alert.get(3), alert.get(4), alert.get(9));
    telegram.send_to_owner(owner, |offset| alert_text(&market, &condition, level, value, start, note.as_deref(), at, offset)).await;
}

// ---------------------------------------------------------------- the API

/// `GET /api/telegram`: whether the bot is set up, and this browser's chats.
pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let Some(telegram) = state.telegram.as_ref() else {
        return Ok(Json(json!({ "enabled": false, "bot": null, "chats": [] })));
    };
    let chats = telegram.chats(&owner).await?;
    Ok(Json(json!({
        "enabled": true,
        "bot": telegram.bot(),
        "maxClearHours": MAX_CLEAR_HOURS,
        "chats": chats.iter().map(|c| json!({ "chatId": c.chat_id, "name": c.name, "linkedAt": c.linked_at, "offsetMinutes": c.utc_offset, "zone": c.zone, "clearAfterHours": c.clear_after_hours })).collect::<Vec<_>>(),
    })))
}

fn enabled(state: &AppState) -> ApiResult<&Arc<Telegram>> {
    state.telegram.as_ref().ok_or_else(|| ApiError(axum::http::StatusCode::SERVICE_UNAVAILABLE, "this server has no Telegram bot set up".into()))
}

/// The clock a request carries: `{offsetMinutes, zone}`, both optional.
fn clock(body: &Value) -> (i32, Option<&str>) {
    let offset = body["offsetMinutes"].as_i64().unwrap_or(0).clamp(i64::from(-MAX_OFFSET), i64::from(MAX_OFFSET)) as i32;
    let zone = body["zone"].as_str().filter(|z| !z.is_empty() && z.len() <= 64);
    (offset, zone)
}

/// `POST /api/telegram/link`: a fresh code and the link that spends it. The
/// body carries the reader's clock, so the first message already reads right.
pub async fn link(State(state): State<AppState>, headers: HeaderMap, body: Option<Json<Value>>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let body = body.map_or(Value::Null, |Json(b)| b);
    let (offset, zone) = clock(&body);
    let (code, url) = enabled(&state)?.link(&owner, offset, zone).await?;
    Ok(Json(json!({ "code": code, "url": url, "goodForMs": CODE_GOOD_FOR })))
}

/// `PATCH /api/telegram`: the reader's zone changed, or summer time moved it,
/// or they chose how long a message stays in the chat.
pub async fn set_clock(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let telegram = enabled(&state)?;
    let mut changed = 0;
    if body.get("offsetMinutes").is_some() || body.get("zone").is_some() {
        let (offset, zone) = clock(&body);
        changed = telegram.set_clock(&owner, offset, zone).await?;
    }
    if let Some(hours) = body.get("clearAfterHours") {
        let hours = match hours {
            Value::Null => None,
            v => Some(v.as_i64().filter(|h| *h > 0).ok_or_else(|| ApiError::bad_request("clearAfterHours must be hours above zero, or null to keep messages"))? as i32),
        };
        changed = telegram.set_clearing(&owner, hours).await?;
    }
    Ok(Json(json!({ "chats": changed })))
}

/// `POST /api/telegram/test`: one message, to see that it arrives.
pub async fn test(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let telegram = enabled(&state)?;
    let chats = telegram.chats(&owner).await?;
    if chats.is_empty() {
        return Err(ApiError::bad_request("no chat is linked to this browser yet"));
    }
    let now = now_ms();
    telegram.send_to_owner(&owner, |offset| format!("tradoor test message — {}. Alerts will arrive like this.", stamp(now, offset))).await;
    Ok(Json(json!({ "sent": chats.len() })))
}

/// `DELETE /api/telegram/{chat_id}`: stop sending to one chat.
pub async fn unlink(State(state): State<AppState>, headers: HeaderMap, Path(chat_id): Path<i64>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let gone = enabled(&state)?.unlink(&owner, chat_id).await?;
    Ok(Json(json!({ "unlinked": gone })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alert_reads_the_way_the_page_writes_it() {
        let at = tradoor_core::time::parse_iso("2026-09-24T22:05:00Z").unwrap();
        let text = alert_text("binance-perp:BTCUSDT", "crossing", 83_520.5, 83_734.9, None, None, at, 0);
        assert_eq!(text, "BTCUSDT.P Crossing 83,520.50\nfired at 83,734.90 — 2026-09-24 22:05 UTC", "a bare crossing with nothing to compare says no way");
        let noted = alert_text("bybit-spot:SOLUSDT", "greater", 114.8, 115.2, None, Some("  take the swing  "), at, 0);
        assert_eq!(noted, "📈 SOLUSDT Greater Than 114.80\nfired at 115.20 — 2026-09-24 22:05 UTC\ntake the swing");
        let fell = alert_text("binance-perp:BTCUSDT", "crossing", 80_000.0, 79_990.0, Some(83_000.0), None, at, 0);
        assert!(fell.starts_with("📉 BTCUSDT.P Crossing"), "set below the price and reached: a fall");
    }

    #[test]
    fn a_message_is_written_in_the_readers_own_hours() {
        let at = tradoor_core::time::parse_iso("2026-09-24T22:05:00Z").unwrap();
        assert_eq!(stamp(at, 8 * 60), "2026-09-25 06:05 UTC+8", "a day later where they are");
        assert_eq!(stamp(at, -4 * 60), "2026-09-24 18:05 UTC-4");
        assert_eq!(stamp(at, 5 * 60 + 30), "2026-09-25 03:35 UTC+5:30");
        assert_eq!(stamp(at, 0), "2026-09-24 22:05 UTC");
        assert_eq!(stamp(at, 99 * 60), stamp(at, MAX_OFFSET), "nothing beyond a real offset is one");
        assert!(alert_text("binance-perp:BTCUSDT", "less", 1.0, 0.9, None, None, at, 7 * 60).ends_with("2026-09-25 05:05 UTC+7"));
    }

    #[test]
    fn the_bot_reads_a_code_and_nothing_else() {
        assert_eq!(start_code("/start ab23cd45"), Some("ab23cd45"));
        assert_eq!(start_code("/start@tradoor_bot ab23cd45"), Some("ab23cd45"));
        assert_eq!(start_code("/start"), None, "no code: the bot explains itself instead");
        assert_eq!(start_code("hello"), None);
        assert_eq!(start_code(&format!("/start {}", "x".repeat(65))), None, "nothing that long is a code of ours");
    }

    #[test]
    fn updates_are_read_and_the_rest_ignored() {
        let result = json!([
            { "update_id": 7, "message": { "chat": { "id": 42, "username": "ricky" }, "text": "/start abc" } },
            { "update_id": 8, "message": { "chat": { "id": 43, "first_name": "Olivia" } } },
            { "update_id": 9, "channel_post": { "chat": { "id": 44 }, "text": "hi" } },
        ]);
        let updates = read_updates(&result);
        assert_eq!(updates.len(), 2, "a channel post is not a message to the bot");
        assert_eq!(updates[0], Incoming { id: 7, chat_id: 42, name: Some("ricky".into()), text: "/start abc".into() });
        assert_eq!(updates[1].text, "", "a message with no text still names its chat");
        assert_eq!(read_updates(&json!({})), vec![]);
    }

    #[test]
    fn a_code_is_short_unguessable_and_easy_to_read() {
        let code = fresh_code();
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert!(!code.contains('l') && !code.contains('0') && !code.contains('1'), "letters a digit could be mistaken for are left out");
        assert_ne!(code, fresh_code());
    }

    /// Needs a Postgres at DATABASE_URL with the schema applied; run with
    /// `cargo test -p tradoor-server -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a Postgres at DATABASE_URL"]
    async fn a_code_links_one_chat_once() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = deadpool_postgres::Config { url: Some(url), ..Default::default() }.create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls).unwrap();
        const OWNER: &str = "zz-test-telegram-01";
        let chat = -4_242_424_242;
        let client = pool.get().await.unwrap();
        client.execute("DELETE FROM telegram_chat WHERE owner = $1 OR chat_id = $2", &[&OWNER, &chat]).await.unwrap();
        client.execute("DELETE FROM telegram_link WHERE owner = $1", &[&OWNER]).await.unwrap();

        let code = mint_code(&pool, OWNER, 8 * 60, Some("Asia/Singapore")).await.unwrap();
        let update = Incoming { id: 1, chat_id: chat, name: Some("ricky".into()), text: format!("/start {code}") };
        assert!(claim(&pool, &code, &update).await.unwrap(), "the code links the chat");
        assert!(!claim(&pool, &code, &update).await.unwrap(), "and is spent");
        assert!(!claim(&pool, "neverminted", &update).await.unwrap());

        let rows = client.query("SELECT chat_id, name, utc_offset, zone FROM telegram_chat WHERE owner = $1", &[&OWNER]).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].get::<_, i64>(0), rows[0].get::<_, Option<String>>(1)), (chat, Some("ricky".into())));
        assert_eq!((rows[0].get::<_, i32>(2), rows[0].get::<_, Option<String>>(3)), (8 * 60, Some("Asia/Singapore".into())), "the clock the link carried");

        // A message kept for clearing goes when its time is up, and only then.
        remember(&pool, chat, 501, now_ms() - 1_000).await.unwrap();
        remember(&pool, chat, 502, now_ms() + 3_600_000).await.unwrap();
        let due = take_due(&pool).await.unwrap();
        assert_eq!(due, [(chat, 501)], "only the one whose time has passed");
        assert_eq!(take_due(&pool).await.unwrap(), [], "and it is taken once");
        client.execute("DELETE FROM telegram_message WHERE chat_id = $1", &[&chat]).await.unwrap();

        assert_eq!(forget(&pool, chat).await.unwrap(), 1, "/stop ends it");
        let left: i64 = client.query_one("SELECT count(*) FROM telegram_chat WHERE owner = $1", &[&OWNER]).await.unwrap().get(0);
        assert_eq!(left, 0);
        client.execute("DELETE FROM telegram_link WHERE owner = $1", &[&OWNER]).await.unwrap();
    }

    #[test]
    fn the_token_never_reaches_a_log_line() {
        let token = "123456:AAHsecret";
        assert_eq!(strip_token(&format!("error sending request for url (https://api.telegram.org/bot{token}/getMe)"), token), "error sending request for url (https://api.telegram.org/bot…/getMe)");
    }
}
