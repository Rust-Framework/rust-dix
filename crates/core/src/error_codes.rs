//! Stable error codes for the framework-wide error contract.
//!
//! Every error produced by the framework carries a machine-readable
//! [`AppError::code`](crate::error::AppError::code) in addition to its HTTP
//! status: clients branch on the code instead of parsing a human-readable
//! message (one status can have several causes).
//!
//! [`ERROR_CODES`] is the registry of the codes the framework itself emits. It
//! exists so the contract can be documented, generated and tested:
//! [`AppError::code`](crate::error::AppError::code) and
//! [`default_code_for_status`] are the runtime sources, and this table must
//! contain every reachable code exactly once with the same status/retryability.
//! A new code cannot be added to either function without registering it here.
//!
//! Rules for new entries (see `docs/design/error-codes.md` in the rust-webx
//! repository):
//!
//! * lowercase `snake_case`; stable once released — later versions add entries,
//!   never rename or remove them;
//! * one HTTP status per code, and `retryable` must match
//!   [`status_is_retryable`] for that status, except for a documented
//!   condition-specific code (`conflict_retryable`: a `409` whose conflict is
//!   transient);
//! * application-defined codes (carried through
//!   [`AppError::new`](crate::error::AppError::new) or
//!   [`AppError::with_extension`](crate::error::AppError::with_extension)) are
//!   **not** registered here: they belong to the application's own contract.

/// One entry of the stable error-code registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCodeSpec {
    /// The `code` extension value carried by `application/problem+json`.
    pub code: &'static str,
    /// The HTTP status the code normally implies.
    ///
    /// [`FALLBACK_STATUS`] for the generic `error` fallback, whose actual status
    /// is carried by the error itself (see
    /// [`AppError::status`](crate::error::AppError::status)).
    pub status: u16,
    /// Whether retrying the same request may succeed.
    ///
    /// `true` for transient conditions: every `429`/`502`/`503`/`504` status,
    /// plus `conflict_retryable` — a `409` that means "wait for the conflicting
    /// work, then retry". A validation failure, a permanent state conflict or a
    /// server-side defect is never retryable.
    pub retryable: bool,
}

/// `status` sentinel for [`ErrorCodeSpec`] entries whose status is carried by
/// the error instead of being fixed by the code.
pub const FALLBACK_STATUS: u16 = 0;

/// Every stable code emitted by the framework.
///
/// Ordered by HTTP status, then by code, for readable diffs and docs. Look a
/// code up with [`ERROR_CODES`] or a plain `find`; the list is a `const` table,
/// not a runtime map.
///
/// ```
/// use rust_core::error_codes::{ErrorCodeSpec, ERROR_CODES};
///
/// let spec = ERROR_CODES
///     .iter()
///     .find(|spec| spec.code == "service_unavailable")
///     .expect("registered");
/// assert_eq!(
///     *spec,
///     ErrorCodeSpec {
///         code: "service_unavailable",
///         status: 503,
///         retryable: true,
///     }
/// );
/// ```
pub const ERROR_CODES: &[ErrorCodeSpec] = &[
    // --- 4xx: client errors -------------------------------------------------
    ErrorCodeSpec {
        code: "bad_request",
        status: 400,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "invalid_json",
        status: 400,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "validation_failed",
        status: 400,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "unauthorized",
        status: 401,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "forbidden",
        status: 403,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "not_found",
        status: 404,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "route_not_found",
        status: 404,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "method_not_allowed",
        status: 405,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "conflict",
        status: 409,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "conflict_retryable",
        status: 409,
        retryable: true,
    },
    ErrorCodeSpec {
        code: "payload_too_large",
        status: 413,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "unsupported_media_type",
        status: 415,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "unprocessable_entity",
        status: 422,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "too_many_requests",
        status: 429,
        retryable: true,
    },
    // --- 5xx: server errors -------------------------------------------------
    ErrorCodeSpec {
        code: "dependency_injection",
        status: 500,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "internal_error",
        status: 500,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "invalid_state",
        status: 500,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "response_serialization",
        status: 500,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "not_implemented",
        status: 501,
        retryable: false,
    },
    ErrorCodeSpec {
        code: "bad_gateway",
        status: 502,
        retryable: true,
    },
    ErrorCodeSpec {
        code: "service_unavailable",
        status: 503,
        retryable: true,
    },
    ErrorCodeSpec {
        code: "gateway_timeout",
        status: 504,
        retryable: true,
    },
    // --- Fallback -----------------------------------------------------------
    ErrorCodeSpec {
        code: "error",
        status: FALLBACK_STATUS,
        retryable: false,
    },
];

/// Stable machine-readable code for a bare HTTP status.
///
/// Used when an error carries no more specific code: a raw status supplied to
/// [`AppError::status`](crate::error::AppError::status), or a status that has no
/// dedicated constructor. Unknown statuses fall back to `"error"`.
pub fn default_code_for_status(status: u16) -> &'static str {
    match status {
        400 => "bad_request",
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        405 => "method_not_allowed",
        409 => "conflict",
        413 => "payload_too_large",
        415 => "unsupported_media_type",
        422 => "unprocessable_entity",
        429 => "too_many_requests",
        500 => "internal_error",
        501 => "not_implemented",
        502 => "bad_gateway",
        503 => "service_unavailable",
        504 => "gateway_timeout",
        _ => "error",
    }
}

/// Whether a bare HTTP status describes a transient condition worth retrying.
///
/// Only `429` (rate limited) and `502`/`503`/`504` (upstream unavailable or
/// timed out) qualify. A `409` is condition-specific: it is only retryable when
/// it was built with
/// [`AppError::conflict_retryable`](crate::error::AppError::conflict_retryable).
pub fn status_is_retryable(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}
