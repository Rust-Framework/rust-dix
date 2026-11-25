#![forbid(unsafe_code)]

//! `rust-core` — the framework-neutral contract layer of the Rust Framework.
//!
//! This crate is the single home of the types that cross framework boundaries:
//!
//! * the application error carrier ([`AppError`] / [`ErrorKind`] /
//!   [`FieldProblem`]), the [`Result`] alias and the stable error-code registry
//!   ([`error_codes::ERROR_CODES`]);
//! * pagination contracts: [`PagedRequest`] / [`PagedResult`] (offset) and
//!   [`KeysetRequest`] / [`Cursor`] / [`CursorPage`] / [`CursorError`] (keyset);
//! * the business result envelope [`OperationResult`];
//! * the persistence vocabulary [`IPatchable`] (incremental entity updates).
//!
//! It deliberately contains no web, ORM or DI vocabulary: `rust-webx`,
//! `rust-ef` and `rust-dix` keep their own diagnostic types and convert into
//! [`AppError`] at their boundaries. That keeps the dependency direction
//! `framework → rust-core` one-way and lets the frameworks version
//! independently.
//!
//! `rust-webx` only **re-exports** these contract types (keeping the
//! `webx_core::pagination` / `webx_core::keyset` / `webx_core::operation` paths
//! usable); it must not define a second copy. `rust-ef` consumes
//! [`PagedRequest`]/[`PagedResult`] directly for its `to_page` terminal, which is
//! why the types live here instead of in the web layer.
//!
//! # Quick start
//!
//! ```rust
//! use rust_core::{AppError, ErrorKind};
//!
//! let error = AppError::not_found("order '42' does not exist");
//! assert_eq!(error.kind(), ErrorKind::NotFound);
//! assert_eq!(error.status_code(), 404);
//! assert_eq!(error.code(), "not_found");
//! assert_eq!(error.public_detail(), Some("order '42' does not exist"));
//! assert!(!error.retryable());
//! ```
//!
//! `detail` is client-visible, `operator` is log-only: use
//! [`AppError::internal`] for server defects so that internals never reach the
//! wire.
//!
//! ```
//! use rust_core::{PagedRequest, PagedResult};
//!
//! let paging = PagedRequest { page: 3, page_size: 20 };
//! assert_eq!(paging.offset(), 40);
//!
//! // Entities do not have to be `Serialize`; only the mapped DTO page does.
//! let page = PagedResult::new(vec![1_u32, 2, 3], 42).map_items(|id| id.to_string());
//! assert_eq!(page.total, 42);
//! ```

pub mod error;
pub mod error_codes;
pub mod keyset;
pub mod operation;
pub mod pagination;
pub mod persistence;

pub use error::{AppError, ErrorKind, FieldProblem, Result};
pub use error_codes::{
    default_code_for_status, status_is_retryable, ErrorCodeSpec, ERROR_CODES, FALLBACK_STATUS,
};
pub use keyset::{Cursor, CursorError, CursorPage, KeysetRequest, MAX_CURSOR_BYTES};
pub use operation::OperationResult;
pub use pagination::{PagedRequest, PagedResult, DEFAULT_PAGE, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};
pub use persistence::IPatchable;
