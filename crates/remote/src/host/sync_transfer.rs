//! Port of host/sync-transfer.ts.
//!
//! Offsets and lengths count UTF-16 code units, as the TypeScript host and
//! renderer did, so a desktop reads pieces with `offset += data.length`.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

use super::protocol::{SessionSyncChunk, SessionSyncTransfer};
use super::store::{StoredSync, now_ms};

// The desktop rejects host responses over 16 MiB. Syncs above the inline limit
// are served as pieces of one serialized revision, each well under that cap
// after JSON string escaping, so neither a long transcript nor a single huge
// block can produce an oversized response.
pub const INLINE_SYNC_BYTES: usize = 4 * 1024 * 1024;
pub const SYNC_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const TRANSFER_TTL_MS: i64 = 2 * 60_000;
const MAX_TRANSFERS: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct SyncLimits {
    pub inline: usize,
    pub chunk: usize,
}

impl Default for SyncLimits {
    fn default() -> Self {
        Self {
            inline: INLINE_SYNC_BYTES,
            chunk: SYNC_CHUNK_BYTES,
        }
    }
}

struct Transfer {
    id: String,
    session_id: String,
    text: Vec<u16>,
    expires: i64,
}

/// What `sessions.sync` answers: the sync itself, already serialized, or a
/// transfer to read in pieces.
#[derive(Debug, Clone)]
pub enum SyncResponse {
    /// Serialized `SessionSync` JSON.
    Inline(String),
    Chunked(SessionSyncTransfer),
}

impl Serialize for SyncResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Inline(text) => serde_json::value::RawValue::from_string(text.clone())
                .map_err(serde::ser::Error::custom)?
                .serialize(serializer),
            Self::Chunked(transfer) => {
                #[derive(Serialize)]
                struct Chunked<'a> {
                    kind: &'static str,
                    transfer: &'a str,
                    length: i64,
                }
                Chunked {
                    kind: "chunked",
                    transfer: &transfer.transfer,
                    length: transfer.length,
                }
                .serialize(serializer)
            }
        }
    }
}

pub struct SyncTransfers {
    limits: SyncLimits,
    transfers: Mutex<VecDeque<Transfer>>,
    clock: fn() -> i64,
}

impl Default for SyncTransfers {
    fn default() -> Self {
        Self::new(SyncLimits::default())
    }
}

/// `Buffer.byteLength(JSON.stringify(text))` for UTF-16 text.
pub fn encoded_bytes(units: &[u16]) -> usize {
    let mut size = 2;
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        size += match unit {
            0x22 | 0x5c => 2,
            0x08 | 0x09 | 0x0a | 0x0c | 0x0d => 2,
            0x00..=0x1f => 6,
            0x20..=0x7f => 1,
            0x80..=0x7ff => 2,
            0xd800..=0xdbff
                if units
                    .get(index + 1)
                    .is_some_and(|next| (0xdc00..=0xdfff).contains(next)) =>
            {
                index += 1;
                4
            }
            // JSON.stringify escapes a lone surrogate as `\uXXXX`.
            0xd800..=0xdfff => 6,
            _ => 3,
        };
        index += 1;
    }
    size
}

impl SyncTransfers {
    pub fn new(limits: SyncLimits) -> Self {
        Self {
            limits,
            transfers: Mutex::new(VecDeque::new()),
            clock: now_ms,
        }
    }

    pub fn respond(&self, session_id: &str, sync: &StoredSync) -> Result<SyncResponse, String> {
        let text = serde_json::to_string(sync).map_err(|error| error.to_string())?;
        // UTF-8 byte length is what the desktop's response cap measures.
        if matches!(sync, StoredSync::Unchanged { .. }) || text.len() <= self.limits.inline {
            return Ok(SyncResponse::Inline(text));
        }
        let units: Vec<u16> = text.encode_utf16().collect();
        drop(text);
        let now = (self.clock)();
        let mut transfers = self
            .transfers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        transfers.retain(|entry| entry.expires >= now);
        while transfers.len() >= MAX_TRANSFERS {
            transfers.pop_front();
        }
        let transfer = uuid::Uuid::new_v4().to_string();
        let length = units.len() as i64;
        transfers.push_back(Transfer {
            id: transfer.clone(),
            session_id: session_id.into(),
            text: units,
            expires: now + TRANSFER_TTL_MS,
        });
        Ok(SyncResponse::Chunked(SessionSyncTransfer {
            transfer,
            length,
        }))
    }

    /// `offset` is the request's `Number(params.offset)`.
    pub fn chunk(
        &self,
        session_id: &str,
        transfer: &str,
        offset: f64,
    ) -> Result<SessionSyncChunk, String> {
        let now = (self.clock)();
        let mut transfers = self
            .transfers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(position) = transfers.iter().position(|entry| entry.id == transfer) else {
            return Err("Session transfer expired; reload the session".into());
        };
        let entry = &mut transfers[position];
        if entry.session_id != session_id || entry.expires < now {
            return Err("Session transfer expired; reload the session".into());
        }
        if !super::js::is_safe_integer_f64(offset)
            || offset < 0.0
            || offset > entry.text.len() as f64
        {
            return Err("Invalid session transfer offset".into());
        }
        let offset = offset as usize;
        let length = entry.text.len();
        // Transcript JSON usually encodes near one byte per UTF-16 unit; shrink
        // the piece when escaping or non-ASCII text makes it larger.
        let mut end = length.min(offset + 1.max(self.limits.chunk * 2 / 3));
        while end - offset > 1 && encoded_bytes(&entry.text[offset..end]) > self.limits.chunk {
            end = offset + (end - offset).div_ceil(2);
        }
        // Never split a surrogate pair: a lone surrogate is not valid JSON text
        // for the desktop's native parser.
        if end < length && end - offset > 1 && (0xd800..=0xdbff).contains(&entry.text[end - 1]) {
            end -= 1;
        }
        let data = String::from_utf16_lossy(&entry.text[offset..end]);
        entry.expires = now + TRANSFER_TTL_MS;
        if end == length {
            transfers.remove(position);
        }
        Ok(SessionSyncChunk { data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::{HostSession, SessionSync};
    use serde_json::json;
    use std::sync::Arc;

    fn sync(text: &str) -> StoredSync {
        let value: HostSession = serde_json::from_value(json!({
            "projectId": "project",
            "revision": 2,
            "status": "idle",
            "updatedAt": 0,
            "blockRevisions": { "block": 2 },
            "session": {
                "id": "session",
                "harness": "codex",
                "model": "codex:test",
                "modelSettings": {},
                "runtimeMode": "supervised",
                "cwd": "/repo",
                "title": "Session",
                "blocks": [{ "id": "block", "role": "assistant", "text": text }]
            }
        }))
        .unwrap();
        StoredSync::Delta {
            base: 1,
            value: Arc::new(value),
        }
    }

    fn read(transfers: &SyncTransfers, transfer: &str, length: i64) -> Vec<String> {
        let mut pieces = Vec::new();
        let mut offset = 0;
        while offset < length {
            let data = transfers
                .chunk("session", transfer, offset as f64)
                .unwrap()
                .data;
            offset += monocode_core::js::len(&data) as i64;
            pieces.push(data);
        }
        pieces
    }

    #[test]
    fn returns_small_syncs_inline() {
        let transfers = SyncTransfers::new(SyncLimits {
            inline: 1024,
            chunk: 256,
        });
        let value = sync("short");
        match transfers.respond("session", &value).unwrap() {
            SyncResponse::Inline(text) => {
                assert_eq!(text, serde_json::to_string(&value).unwrap());
                let parsed: SessionSync = serde_json::from_str(&text).unwrap();
                assert!(matches!(parsed, SessionSync::Delta { base: 1, .. }));
            }
            other => panic!("expected an inline sync, got {other:?}"),
        }
    }

    #[test]
    fn splits_large_syncs_into_bounded_pieces_without_splitting_characters() {
        let transfers = SyncTransfers::new(SyncLimits {
            inline: 1024,
            chunk: 256,
        });
        let value = sync(&"🚀\"\u{1}".repeat(2_000));
        let SyncResponse::Chunked(response) = transfers.respond("session", &value).unwrap() else {
            panic!("Expected a transfer");
        };
        let pieces = read(&transfers, &response.transfer, response.length);
        assert!(pieces.len() > 10);
        for piece in &pieces {
            assert!(serde_json::to_string(piece).unwrap().len() <= 256);
            assert!(!piece.contains('\u{fffd}'));
            let units: Vec<u16> = piece.encode_utf16().collect();
            assert_eq!(
                encoded_bytes(&units),
                serde_json::to_string(piece).unwrap().len()
            );
        }
        let joined: SessionSync = serde_json::from_str(&pieces.concat()).unwrap();
        assert_eq!(
            serde_json::to_value(joined).unwrap(),
            serde_json::to_value(&value).unwrap()
        );
    }

    #[test]
    fn serves_a_transfer_only_for_its_session_and_forgets_it_after_the_last_piece() {
        let transfers = SyncTransfers::new(SyncLimits {
            inline: 64,
            chunk: 4096,
        });
        let SyncResponse::Chunked(response) = transfers
            .respond("session", &sync(&"x".repeat(500)))
            .unwrap()
        else {
            panic!("Expected a transfer");
        };
        assert!(
            transfers
                .chunk("other", &response.transfer, 0.0)
                .unwrap_err()
                .contains("expired")
        );
        assert!(
            transfers
                .chunk("session", &response.transfer, -1.0)
                .unwrap_err()
                .contains("offset")
        );
        read(&transfers, &response.transfer, response.length);
        assert!(
            transfers
                .chunk("session", &response.transfer, 0.0)
                .unwrap_err()
                .contains("expired")
        );
    }

    #[test]
    fn measures_escaped_text_like_json_stringify() {
        let units: Vec<u16> = "a\"\\\n\u{1}é€🚀".encode_utf16().collect();
        // "a" 1, \" 2, \\ 2, \n 2, \u0001 6, é 2, € 3, 🚀 4, quotes 2.
        assert_eq!(encoded_bytes(&units), 24);
        assert_eq!(encoded_bytes(&[0xd83d]), 8);
    }
}
