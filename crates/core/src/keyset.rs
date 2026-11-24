//! Keyset (cursor) pagination contract types.
//!
//! Offset pagination ([`PagedRequest`](crate::pagination::PagedRequest)) shifts
//! when rows are inserted/deleted and gets expensive for deep pages. Keyset
//! pagination asks for "the next N rows after this key" instead:
//!
//! ```ignore
//! // GET /api/messages?cursor=<opaque>&page_size=50
//! let request = KeysetRequest::from_parts(cursor, page_size);
//! let sort_key = request.decoded().unwrap_or_default(); // validate + scope it
//! // WHERE (created_at, id) > (cursor_created_at, cursor_id) ORDER BY ... LIMIT page_size + 1
//! let rows = query_after(sort_key, request.page_size() + 1).await?;
//! let page = CursorPage::from_rows(rows, request.page_size(), |row| {
//!     vec![row.created_at.to_string(), row.id.clone()]
//! });
//! ```
//!
//! The cursor is **opaque and client-supplied**: [`Cursor::decode`] only proves
//! the encoding is well-formed, not that the values are allowed. Validate every
//! component (parse timestamps/ids, reject unknown sort directions, scope the
//! key to the caller's tenant) before using it in a query.
//!
//! A cursor only makes sense for a **stable total order** (for example
//! `(created_at DESC, id DESC)`); without a tie-breaker, rows with equal sort
//! values can be skipped or repeated at page boundaries.
//!
//! `rust-webx` 3.0 only re-exports these types — it must not define a second
//! copy.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::pagination::MAX_PAGE_SIZE;

/// Longest accepted cursor, in bytes (guards against hostile input).
pub const MAX_CURSOR_BYTES: usize = 4_096;

/// Cursor decoding failure.
///
/// The messages are fixed templates: they never echo the cursor value back, so
/// the text is safe to expose to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    /// The cursor is longer than [`MAX_CURSOR_BYTES`].
    TooLong,
    /// The cursor is not valid base64url.
    Encoding,
    /// The decoded payload is not a JSON array of strings.
    Format,
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CursorError::TooLong => "cursor is too long",
            CursorError::Encoding => "cursor is not valid base64url",
            CursorError::Format => "cursor is not a cursor for this endpoint",
        })
    }
}

impl std::error::Error for CursorError {}

/// A malformed client cursor is a request error (`400 validation_failed`), not a
/// server fault.
impl From<CursorError> for AppError {
    fn from(error: CursorError) -> Self {
        AppError::validation(error.to_string())
    }
}

/// One keyset page request.
///
/// Binding: take the two optional query fields and normalize with
/// [`from_parts`](Self::from_parts) (`cursor` absent = first page; `page_size`
/// defaults to `20` and is clamped to `1..=100`). The raw fields stay public and
/// unnormalized; use [`page_size`](Self::page_size) for the effective value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct KeysetRequest {
    /// Opaque cursor for the next page (`None` = first page).
    #[serde(default)]
    pub cursor: Option<String>,
    /// Requested page size (serde default `20`); use
    /// [`page_size`](Self::page_size) for the effective `1..=100` value.
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page_size() -> u32 {
    crate::pagination::DEFAULT_PAGE_SIZE
}

impl KeysetRequest {
    /// Build from an explicit cursor/page size and clamp the size.
    pub fn new(cursor: Option<String>, page_size: u32) -> Self {
        Self { cursor, page_size }.clamped()
    }

    /// Build from the optional query fields of a list endpoint.
    ///
    /// `None` falls back to the framework defaults (no cursor, page size `20`)
    /// and the result is clamped, so the binding layer can hand over
    /// `Option<String>`/`Option<u32>` directly.
    pub fn from_parts(cursor: Option<String>, page_size: Option<u32>) -> Self {
        Self::new(
            cursor,
            page_size.unwrap_or(crate::pagination::DEFAULT_PAGE_SIZE),
        )
    }

    /// Clamp the page size to `1..=100`.
    pub fn clamped(mut self) -> Self {
        self.page_size = self.page_size.clamp(1, MAX_PAGE_SIZE);
        self
    }

    /// Effective page size (`1..=100`).
    pub fn page_size(&self) -> u32 {
        self.page_size.clamp(1, MAX_PAGE_SIZE)
    }

    /// The opaque cursor, if any.
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    /// Decode the cursor into its sort-key components.
    ///
    /// `Ok(Vec::new())` means "first page". The values are untrusted input:
    /// validate them (and scope them to the caller) before a query.
    pub fn decoded(&self) -> Result<Vec<String>, CursorError> {
        match self.cursor() {
            Some(cursor) if !cursor.is_empty() => Cursor::decode(cursor),
            _ => Ok(Vec::new()),
        }
    }
}

/// Opaque cursor codec.
///
/// A cursor is a base64url (no padding) JSON array of strings: one component per
/// stable sort-key column (timestamp, id, ...). It is not signed: tampering only
/// produces values the handler validates anyway.
///
/// ```
/// # use rust_core::keyset::Cursor;
/// let components = ["2026-11-24T00:00:00Z", "42"];
/// let cursor = Cursor::encode(&components);
/// assert_eq!(
///     Cursor::decode(&cursor).unwrap(),
///     vec!["2026-11-24T00:00:00Z".to_string(), "42".to_string()]
/// );
/// ```
pub struct Cursor;

impl Cursor {
    /// Encode sort-key components into a cursor.
    pub fn encode(components: &[&str]) -> String {
        let json = serde_json::to_vec(components).unwrap_or_else(|_| b"[]".to_vec());
        base64url_encode(&json)
    }

    /// Decode a cursor into its sort-key components.
    ///
    /// Rejects input that is too long, not base64url, or not a JSON array of
    /// strings with [`CursorError`]. The decoding is strict: non-canonical
    /// trailing bits are rejected exactly like any other malformed encoding.
    pub fn decode(cursor: &str) -> Result<Vec<String>, CursorError> {
        let cursor = cursor.trim();
        if cursor.len() > MAX_CURSOR_BYTES {
            return Err(CursorError::TooLong);
        }
        let bytes = base64url_decode(cursor).map_err(|_| CursorError::Encoding)?;
        serde_json::from_slice::<Vec<String>>(&bytes).map_err(|_| CursorError::Format)
    }
}

/// One keyset page.
///
/// The struct has **no** `T: Serialize` bound: [`from_entities`](Self::from_entities)
/// accepts entities that are not `Serialize`, and [`map_items`](Self::map_items)
/// maps a page to DTOs before the response is serialized. The `Serialize` derive
/// adds its bound only to the generated `impl<T: Serialize> Serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CursorPage<T> {
    /// Rows of this page.
    pub items: Vec<T>,
    /// Cursor for the next page (`None` = no more rows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Whether another page exists.
    pub has_more: bool,
}

impl<T> CursorPage<T> {
    /// Build a page from items and the cursor that fetches the next page.
    pub fn new(items: Vec<T>, cursor: Option<String>) -> Self {
        Self {
            items,
            has_more: cursor.is_some(),
            cursor,
        }
    }

    /// Build a page from up to `page_size + 1` fetched rows.
    ///
    /// The extra row proves there is a next page; it is dropped from the page and
    /// the last returned row's sort key (built by `sort_key`) becomes the cursor.
    /// Fetching one extra row is what makes `has_more` exact without a `COUNT`.
    pub fn from_rows(
        mut rows: Vec<T>,
        page_size: u32,
        sort_key: impl Fn(&T) -> Vec<String>,
    ) -> Self {
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE) as usize;
        let has_more = rows.len() > page_size;
        if has_more {
            rows.truncate(page_size);
        }
        let cursor = rows.last().filter(|_| has_more).map(|last| {
            let key = sort_key(last);
            let refs: Vec<&str> = key.iter().map(String::as_str).collect();
            Cursor::encode(&refs)
        });
        Self {
            items: rows,
            cursor,
            has_more,
        }
    }

    /// Build a page from up to `page_size + 1` fetched **entities**, mapping each
    /// kept entity to the response item.
    ///
    /// The entity type does not have to be `Serialize`, and the response item
    /// does not have to carry the raw sort values: the cursor is computed from
    /// the last kept entity through `sort_key`, and `map` runs only for the rows
    /// that stay on the page (the probe row is dropped before mapping). Use it
    /// when the list query returns domain entities and the endpoint answers with
    /// DTOs:
    ///
    /// ```ignore
    /// let rows = load_after(&mut self.ctx, &req.room_id, key, paging.page_size() + 1).await?;
    /// Ok(CursorPage::from_entities(rows, paging.page_size(),
    ///     |row| vec![row.created_at.to_string(), row.id.clone()],
    ///     MessageDto::from_model))
    /// ```
    pub fn from_entities<E>(
        mut rows: Vec<E>,
        page_size: u32,
        sort_key: impl Fn(&E) -> Vec<String>,
        map: impl FnMut(E) -> T,
    ) -> Self {
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE) as usize;
        let has_more = rows.len() > page_size;
        if has_more {
            rows.truncate(page_size);
        }
        let cursor = rows.last().filter(|_| has_more).map(|last| {
            let key = sort_key(last);
            let refs: Vec<&str> = key.iter().map(String::as_str).collect();
            Cursor::encode(&refs)
        });
        Self {
            items: rows.into_iter().map(map).collect(),
            cursor,
            has_more,
        }
    }

    /// Map the kept items to another response type, preserving `cursor` and
    /// `has_more`.
    ///
    /// Rebuilding a page by hand (`CursorPage::new(mapped, page.cursor)`)
    /// silently recomputes `has_more` from `cursor.is_some()`; this helper keeps
    /// the page envelope intact and only transforms the items.
    pub fn map_items<U>(self, map: impl FnMut(T) -> U) -> CursorPage<U> {
        CursorPage {
            items: self.items.into_iter().map(map).collect(),
            cursor: self.cursor,
            has_more: self.has_more,
        }
    }
}

// ---------------------------------------------------------------------------
// base64url (RFC 4648 §5, no padding)
//
// The cursor wire format is owned by this crate, and `rust-core` may only depend
// on `serde`/`serde_json`, so the codec is implemented here instead of pulling
// in a base64 dependency. It is deliberately strict: invalid symbols, impossible
// lengths and non-canonical trailing bits are all rejected (the same cases the
// previous `data_encoding::BASE64URL_NOPAD` implementation rejected).
// ---------------------------------------------------------------------------

/// base64url alphabet (`A-Z a-z 0-9 - _`), no padding.
const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let bits = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64URL_ALPHABET[((bits >> 18) & 0x3f) as usize] as char);
        out.push(BASE64URL_ALPHABET[((bits >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64URL_ALPHABET[((bits >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(BASE64URL_ALPHABET[(bits & 0x3f) as usize] as char);
        }
    }
    out
}

fn base64url_decode(input: &str) -> Result<Vec<u8>, ()> {
    let bytes = input.as_bytes();
    // A base64 quantum is 4 symbols; one leftover symbol cannot encode a byte.
    if bytes.len() % 4 == 1 {
        return Err(());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 + 2);
    for chunk in bytes.chunks(4) {
        let mut bits: u32 = 0;
        for &byte in chunk {
            let value = base64url_value(byte).ok_or(())?;
            bits = (bits << 6) | u32::from(value);
        }
        match chunk.len() {
            4 => {
                out.push((bits >> 16) as u8);
                out.push((bits >> 8) as u8);
                out.push(bits as u8);
            }
            3 => {
                // 18 bits carry 2 bytes + 2 trailing bits that must be zero.
                if bits & 0b11 != 0 {
                    return Err(());
                }
                let bits = bits >> 2;
                out.push((bits >> 8) as u8);
                out.push(bits as u8);
            }
            2 => {
                // 12 bits carry 1 byte + 4 trailing bits that must be zero.
                if bits & 0b1111 != 0 {
                    return Err(());
                }
                out.push((bits >> 4) as u8);
            }
            _ => return Err(()),
        }
    }
    Ok(out)
}

fn base64url_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_rfc4648_test_vectors() {
        // RFC 4648 §10, base64url without padding.
        for (bytes, encoded) in [
            (b"".as_slice(), ""),
            (b"f".as_slice(), "Zg"),
            (b"fo".as_slice(), "Zm8"),
            (b"foo".as_slice(), "Zm9v"),
            (b"foob".as_slice(), "Zm9vYg"),
            (b"fooba".as_slice(), "Zm9vYmE"),
            (b"foobar".as_slice(), "Zm9vYmFy"),
        ] {
            assert_eq!(base64url_encode(bytes), encoded);
            assert_eq!(base64url_decode(encoded), Ok(bytes.to_vec()));
        }
        // The url alphabet: '+'/'-' and '/'/'_'.
        assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
        assert_eq!(base64url_decode("-_8"), Ok(vec![0xfb, 0xff]));
    }

    #[test]
    fn base64url_decode_is_strict() {
        // Invalid symbol, padding, impossible length.
        assert_eq!(base64url_decode("not base64!!"), Err(()));
        assert_eq!(base64url_decode("Zg=="), Err(()));
        assert_eq!(base64url_decode("A"), Err(()));
        // Non-canonical trailing bits ("Zh" has 'h' = 100001, trailing 4 bits).
        assert_eq!(base64url_decode("Zh"), Err(()));
        assert_eq!(base64url_decode("Zm9"), Err(()));
        assert_eq!(base64url_decode("Zm8"), Ok(b"fo".to_vec()));
        // Non-ASCII bytes are invalid symbols.
        assert_eq!(base64url_decode("订单"), Err(()));
    }
}
