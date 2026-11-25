//! Offset-pagination contract types.
//!
//! [`PagedRequest`] is the **request** side and [`PagedResult<T>`] the
//! **response** side of classic page/offset pagination; both are framework
//! contracts, not web types. `rust-webx` only re-exports them — it must not
//! define a second copy.
//!
//! There is deliberately **no normalization state** in [`PagedRequest`]:
//! defaults (`1` / `20`) and range validation (`1..=100`) belong to the binding
//! layer, which can reject an out-of-range value with a field error instead of
//! silently clamping it. The constants below are the single source of those
//! defaults and limits.

use serde::{Deserialize, Serialize};

/// Default 1-based page number (the binding layer applies it when the query
/// parameter is absent).
pub const DEFAULT_PAGE: u32 = 1;

/// Default page size (the binding layer applies it when the query parameter is
/// absent).
pub const DEFAULT_PAGE_SIZE: u32 = 20;

/// Largest page size a list endpoint accepts.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Offset-pagination request: which page, how many rows.
///
/// The fields are plain request data. `page` is 1-based; `page_size` must be
/// validated by the binding layer (`1..=`[`MAX_PAGE_SIZE`]) before the handler
/// runs. The type carries **no** defaults and no clamping helpers, so a handler
/// can never observe a half-normalized request: either the binding produced a
/// valid value or the caller received a `400`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PagedRequest {
    /// 1-based page number.
    pub page: u32,
    /// Requested page size (`1..=`[`MAX_PAGE_SIZE`] after binding validation).
    pub page_size: u32,
}

impl PagedRequest {
    /// Zero-based row offset for the SQL `OFFSET` clause.
    ///
    /// The computation is lossless in `u64`: `(u32::MAX - 1) * u32::MAX` is
    /// below `u64::MAX`, so even a hostile `page`/`page_size` pair cannot wrap.
    /// A `page` of `0` saturates to offset `0` instead of underflowing; a
    /// `page_size` of `0` yields offset `0` (the query then returns no rows).
    pub fn offset(&self) -> u64 {
        u64::from(self.page.saturating_sub(1)) * u64::from(self.page_size)
    }

    /// Row limit for the SQL `LIMIT` clause.
    ///
    /// This is the raw request value: the binding layer already validated it to
    /// `1..=`[`MAX_PAGE_SIZE`]. The accessor exists so query code depends on the
    /// contract rather than on the field, and so a future clamp can be added
    /// without touching call sites.
    pub fn limit(&self) -> u32 {
        self.page_size
    }
}

/// One offset-pagination page **and its business outcome, flattened into one
/// struct**.
///
/// The outcome fields (`success`/`status`/`message`) are direct members of the
/// page, not a nested [`OperationResult`](crate::operation::OperationResult):
/// a page is not "an operation that happens to carry items", and composition
/// would leak a second way to reach the same three values (`page.operation.status`
/// vs `page.status`). The Rust shape and the wire shape are therefore identical:
///
/// ```json
/// {"success":true,"status":0,"message":"ok","items":[...],"total":120}
/// ```
///
/// The endpoint can override the outcome with [`with_status`]/[`with_message`]:
///
/// ```ignore
/// let page = query.to_page(&req.paging).await?;          // success/0/"ok"
/// Ok(page.with_status(0).with_message("orders listed"))
/// ```
///
/// The struct has **no** `T: Serialize` bound: a query terminal such as
/// `rust-ef`'s `to_page` returns `PagedResult<Entity>` for entities that are not
/// `Serialize`, and the endpoint maps the kept entities to DTOs with
/// [`map_items`](PagedResult::map_items). The `Serialize` derive adds its bound
/// only to the generated `impl<T: Serialize> Serialize`.
///
/// The page part still carries no request echo: there is no `page`, `page_size`
/// or `total_pages` member; a client computes `total_pages = ceil(total / page_size)`
/// from the request it just sent.
///
/// [`with_status`]: PagedResult::with_status
/// [`with_message`]: PagedResult::with_message
#[derive(Debug, Clone, Serialize)]
pub struct PagedResult<T> {
    /// Whether the operation achieved its goal.
    pub success: bool,
    /// Business status code, defined by the caller (the constructors use `0`
    /// when no explicit code is given). It is not an HTTP status: the transport
    /// layer never reads it.
    pub status: i32,
    /// Human-readable outcome for the caller (UI toast, CLI output, logs).
    pub message: String,
    /// Rows of this page.
    pub items: Vec<T>,
    /// Total number of rows matching the filter, independent of paging.
    pub total: u64,
}

impl<T> PagedResult<T> {
    /// A successful page with the default business status `0` and message `"ok"`.
    pub fn new(items: Vec<T>, total: u64) -> Self {
        Self {
            success: true,
            status: 0,
            message: "ok".to_string(),
            items,
            total,
        }
    }

    /// A failed operation (no rows, total `0`); a failure is not a page.
    pub fn failed(status: i32, message: impl Into<String>) -> Self {
        Self {
            success: false,
            status,
            message: message.into(),
            items: Vec::new(),
            total: 0,
        }
    }

    /// Override the business status (the constructors use `0`).
    pub fn with_status(mut self, status: i32) -> Self {
        self.status = status;
        self
    }

    /// Override the client-facing message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Map the page rows to another response type, keeping `total` and the
    /// operation outcome.
    ///
    /// This is the DTO step of `Entity → Model`: the entity page never has to be
    /// `Serialize`, only the mapped page does.
    pub fn map_items<U>(self, map: impl FnMut(T) -> U) -> PagedResult<U> {
        PagedResult {
            success: self.success,
            status: self.status,
            message: self.message,
            items: self.items.into_iter().map(map).collect(),
            total: self.total,
        }
    }
}
