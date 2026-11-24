//! The framework-wide application error carrier.
//!
//! [`AppError`] is the **single** error type that crosses framework boundaries,
//! is returned by application handlers and is rendered on the wire. The three
//! frameworks keep their own diagnostic types (`RdiError` in rust-dix,
//! `EFError` in rust-ef, and the HTTP/rendering types in rust-webx) and convert
//! into [`AppError`] with `From`/constructors at their edges.
//!
//! # Fields
//!
//! * [`kind`](AppError::kind) — coarse classification; decides the default
//!   status, code and retryability.
//! * [`code`](AppError::code) — stable machine-readable code (snake_case,
//!   non-empty). Application-defined codes are allowed; see
//!   [`crate::error_codes`].
//! * [`detail`](AppError::detail) — client-visible message. `None` means "do
//!   not show a message": the renderer falls back to the status title. Never put
//!   internals here.
//! * [`operator`](AppError::operator) — log-only diagnostic text (the message of
//!   the original defect). Never rendered to the client.
//! * [`retryable`](AppError::retryable) — may the client retry the same request?
//! * `status` — optional status override (the escape hatch for statuses without
//!   a dedicated kind, such as `418`/`422`/`501`). Fields are private so that
//!   adding a field later is not a breaking change.
//! * [`fields`](AppError::fields) — structured per-field problems (validation
//!   and binding failures).
//! * [`extensions`](AppError::extensions) — free-form members attached to the
//!   rendered error object. The rendering layer owns the wire format; this crate
//!   only carries the map.
//!
//! # Retry-After
//!
//! [`AppError::too_many_requests`] stores its wait value in
//! `extensions["x-webx-retry-after"]`. This is the one transport key the shared
//! carrier writes: the value is created by the constructor (so it cannot be
//! reconstructed from `retryable: bool` alone) and the HTTP response layer is
//! responsible for turning it into a `Retry-After` header. Everything else in
//! `extensions` is opaque to this crate.

use std::borrow::Cow;
use std::fmt;

use crate::error_codes::{default_code_for_status, status_is_retryable};

/// Transport extension key carrying the wait value of a typed `429`.
///
/// Written by [`AppError::too_many_requests`] only; read (and stripped from the
/// wire) by the HTTP rendering layer, which owns response-header policy.
const RETRY_AFTER_EXTENSION: &str = "x-webx-retry-after";

/// Coarse error classification shared by the frameworks.
///
/// The kind decides the default HTTP status, machine-readable code and
/// retryability of an [`AppError`]. It is deliberately small: statuses that have
/// no dedicated category use [`Other`](ErrorKind::Other) together with the
/// status override carried by the error.
///
/// `#[non_exhaustive]`: new kinds may be added without a semver-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The client's input is invalid (400): bad request syntax, malformed JSON,
    /// failed validation. The code distinguishes the exact cause.
    InvalidInput,
    /// Authentication is missing or invalid (401).
    Unauthorized,
    /// The authenticated caller is not allowed to do this (403).
    Forbidden,
    /// The addressed resource does not exist (404).
    NotFound,
    /// The request conflicts with the current state (409).
    Conflict,
    /// The request body (or an uploaded file) is too large (413).
    PayloadTooLarge,
    /// The request content type is not supported (415).
    UnsupportedMediaType,
    /// The client exceeded a rate limit (429); retryable.
    RateLimited,
    /// An upstream dependency is unavailable or timed out (502/503/504);
    /// retryable.
    Unavailable,
    /// A server-side defect (500). The client gets no detail.
    Internal,
    /// Dependency injection failed (500). Same wire shape as
    /// [`Internal`](ErrorKind::Internal); kept separate so operators can filter
    /// composition-root defects from handler defects.
    Di,
    /// Any other status, carried by the error's status override (`418`, `422`,
    /// `501`, ...). There is no dedicated kind for every HTTP status.
    Other,
}

impl ErrorKind {
    /// The default HTTP status for this kind.
    ///
    /// [`Other`](ErrorKind::Other) has no natural status: `500` is the safe
    /// fallback for an unclassified error, and callers that mean a specific
    /// status must supply it ([`AppError::status`] or
    /// [`AppError::with_status`]).
    pub fn default_status(self) -> u16 {
        match self {
            ErrorKind::InvalidInput => 400,
            ErrorKind::Unauthorized => 401,
            ErrorKind::Forbidden => 403,
            ErrorKind::NotFound => 404,
            ErrorKind::Conflict => 409,
            ErrorKind::PayloadTooLarge => 413,
            ErrorKind::UnsupportedMediaType => 415,
            ErrorKind::RateLimited => 429,
            ErrorKind::Unavailable => 503,
            ErrorKind::Internal | ErrorKind::Di | ErrorKind::Other => 500,
        }
    }

    /// The default machine-readable code for this kind.
    ///
    /// Condition-specific codes are set by their constructors instead:
    /// [`AppError::status`] uses [`default_code_for_status`], and
    /// [`AppError::conflict_retryable`] uses `conflict_retryable`.
    pub fn default_code(self) -> &'static str {
        match self {
            ErrorKind::InvalidInput => "bad_request",
            ErrorKind::Unauthorized => "unauthorized",
            ErrorKind::Forbidden => "forbidden",
            ErrorKind::NotFound => "not_found",
            ErrorKind::Conflict => "conflict",
            ErrorKind::PayloadTooLarge => "payload_too_large",
            ErrorKind::UnsupportedMediaType => "unsupported_media_type",
            ErrorKind::RateLimited => "too_many_requests",
            ErrorKind::Unavailable => "service_unavailable",
            ErrorKind::Internal => "internal_error",
            ErrorKind::Di => "dependency_injection",
            ErrorKind::Other => "error",
        }
    }

    /// The default retryability for this kind.
    ///
    /// Only [`RateLimited`](ErrorKind::RateLimited) and
    /// [`Unavailable`](ErrorKind::Unavailable) are transient. A `409` is only
    /// retryable when it was built as
    /// [`AppError::conflict_retryable`] — an explicit override.
    pub fn default_retryable(self) -> bool {
        matches!(self, ErrorKind::RateLimited | ErrorKind::Unavailable)
    }

    /// Classify a bare HTTP status.
    ///
    /// `502`/`503`/`504` map to [`Unavailable`](ErrorKind::Unavailable),
    /// `501` and every other status without a dedicated category (`418`, `422`,
    /// ...) to [`Other`](ErrorKind::Other), and the remaining `5xx` statuses to
    /// [`Internal`](ErrorKind::Internal).
    pub fn from_status(status: u16) -> Self {
        match status {
            400 => ErrorKind::InvalidInput,
            401 => ErrorKind::Unauthorized,
            403 => ErrorKind::Forbidden,
            404 => ErrorKind::NotFound,
            409 => ErrorKind::Conflict,
            413 => ErrorKind::PayloadTooLarge,
            415 => ErrorKind::UnsupportedMediaType,
            429 => ErrorKind::RateLimited,
            500 => ErrorKind::Internal,
            501 => ErrorKind::Other,
            502..=504 => ErrorKind::Unavailable,
            505..=599 => ErrorKind::Internal,
            _ => ErrorKind::Other,
        }
    }
}

/// One structured per-field problem (validation / binding failure).
///
/// Fields are public so the rendering layer can group them into the wire
/// `errors` object without extra accessor churn. `code` is optional structured
/// classification (`"invalid_format"`, ...): it is metadata for the caller and
/// is not rendered as part of the grouped messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldProblem {
    /// Name of the offending field.
    pub field: String,
    /// Client-visible explanation.
    pub message: String,
    /// Optional machine-readable classification for this field problem.
    pub code: Option<String>,
}

impl FieldProblem {
    /// A field problem with no classification code.
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code: None,
        }
    }

    /// Attach a machine-readable classification code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

/// The framework-wide application error.
///
/// Build it with a semantic constructor ([`AppError::not_found`],
/// [`AppError::internal`], ...) or [`AppError::new`] for a custom kind/code, and
/// refine it with the `with_*` builders. Fields are private on purpose: the
/// type is a long-lived contract, and adding a field must not be a breaking
/// change.
#[derive(Debug)]
pub struct AppError {
    kind: ErrorKind,
    code: Cow<'static, str>,
    detail: Option<String>,
    operator: Option<String>,
    retryable: bool,
    status: Option<u16>,
    fields: Vec<FieldProblem>,
    extensions: serde_json::Map<String, serde_json::Value>,
}

impl AppError {
    /// Build an error from an explicit kind and code.
    ///
    /// `code` must be non-empty; an empty code is replaced by
    /// [`ErrorKind::default_code`] so the invariant "the wire always carries a
    /// code" holds. The status defaults to [`ErrorKind::default_status`].
    pub fn new(kind: ErrorKind, code: impl Into<Cow<'static, str>>) -> Self {
        let code = code.into();
        let code = if code.is_empty() {
            Cow::Borrowed(kind.default_code())
        } else {
            code
        };
        Self {
            kind,
            code,
            detail: None,
            operator: None,
            retryable: kind.default_retryable(),
            status: None,
            fields: Vec::new(),
            extensions: serde_json::Map::new(),
        }
    }

    /// A `400` validation failure with a client-visible message.
    ///
    /// For per-field validation use [`AppError::with_field`] on the result.
    pub fn validation(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, "validation_failed").with_detail(detail)
    }

    /// A generic `400` bad request.
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, "bad_request").with_detail(detail)
    }

    /// A `401` authentication failure.
    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unauthorized, "unauthorized").with_detail(detail)
    }

    /// A `403` authorization failure.
    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, "forbidden").with_detail(detail)
    }

    /// A `404` for a missing resource.
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, "not_found").with_detail(detail)
    }

    /// A `404` for a request path that no route matched.
    pub fn route_not_found(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, "route_not_found").with_detail(detail)
    }

    /// A permanent state/version `409`; not retryable.
    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, "conflict").with_detail(detail)
    }

    /// A transient `409`: the client may retry once the conflicting work
    /// finishes.
    ///
    /// This is a "wait, then retry" signal, not an instruction to retry blindly:
    /// the client should re-read the state (or replay the same idempotency key)
    /// instead of hammering the endpoint. No `Retry-After` value is carried,
    /// because the framework does not know how long the other work takes.
    pub fn conflict_retryable(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, "conflict_retryable")
            .with_detail(detail)
            .with_retryable(true)
    }

    /// A `413` for a body or upload exceeding the configured limit.
    pub fn payload_too_large(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::PayloadTooLarge, "payload_too_large").with_detail(detail)
    }

    /// A `415` for an unsupported request content type.
    pub fn unsupported_media_type(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnsupportedMediaType, "unsupported_media_type").with_detail(detail)
    }

    /// A typed `429` that carries its `Retry-After` value.
    ///
    /// Prefer this over [`AppError::status(429, …)`](AppError::status): the
    /// response layer writes `Retry-After: <retry_after_secs>` for you, so
    /// business code never formats the header or hand-writes the status code.
    pub fn too_many_requests(retry_after_secs: u64, detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::RateLimited, "too_many_requests")
            .with_detail(detail)
            .with_extension(RETRY_AFTER_EXTENSION, retry_after_secs)
    }

    /// A `503` for a transient upstream/service outage; retryable.
    ///
    /// `detail` must be safe to show to the client. For a server-side defect
    /// that happens to be classified `503` use
    /// [`AppError::status(503, detail)`](AppError::status) /
    /// [`AppError::internal`] instead.
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, "service_unavailable").with_detail(detail)
    }

    /// A `500` server-side defect. The message is log-only (`operator`), so
    /// internals never reach the client.
    pub fn internal(operator: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, "internal_error").with_operator(operator)
    }

    /// A `500` dependency-injection failure. The message is log-only.
    ///
    /// Same wire shape as [`AppError::internal`], separate kind so operators can
    /// filter composition-root defects from handler defects.
    pub fn di(operator: impl Into<String>) -> Self {
        Self::new(ErrorKind::Di, "dependency_injection").with_operator(operator)
    }

    /// A `500` invalid-state / programming error (`invalid_state`). The message
    /// is log-only.
    pub fn state(operator: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, "invalid_state").with_operator(operator)
    }

    /// A `500` "the response could not be serialized" defect.
    ///
    /// Kept distinct from [`From<serde_json::Error>`] (which stays on the request
    /// parsing path and maps to `400 invalid_json`): a DTO that cannot be
    /// serialized blames the server, not the client. There is no `From` for this
    /// conversion, so call sites opt in explicitly.
    pub fn response_serialization(error: serde_json::Error) -> Self {
        Self::new(ErrorKind::Internal, "response_serialization").with_operator(error.to_string())
    }

    /// A `400` for a request body that is not valid JSON.
    ///
    /// Equivalent to the [`From<serde_json::Error>`] conversion; use whichever
    /// reads better. `detail` is deliberately `None`: the renderer falls back to
    /// the status title.
    pub fn invalid_json(error: serde_json::Error) -> Self {
        Self::new(ErrorKind::InvalidInput, "invalid_json").with_operator(error.to_string())
    }

    /// A status-first error: the escape hatch for statuses without a dedicated
    /// constructor (`418`, `422`, `501`, runtime-decided statuses, ...).
    ///
    /// The kind is derived with [`ErrorKind::from_status`], the code with
    /// [`default_code_for_status`], and retryability follows
    /// [`status_is_retryable`].
    pub fn status(status: u16, detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::from_status(status),
            code: Cow::Borrowed(default_code_for_status(status)),
            detail: Some(detail.into()),
            operator: None,
            retryable: status_is_retryable(status),
            status: Some(status),
            fields: Vec::new(),
            extensions: serde_json::Map::new(),
        }
    }

    /// Attach a client-visible detail message.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Attach a log-only diagnostic message.
    pub fn with_operator(mut self, operator: impl Into<String>) -> Self {
        self.operator = Some(operator.into());
        self
    }

    /// Override the HTTP status (the escape hatch for statuses without a
    /// dedicated kind).
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Make the error explicitly retryable or non-retryable.
    ///
    /// The explicit value always wins over the kind/status default; this is how
    /// [`AppError::conflict_retryable`] expresses a transient `409`.
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Attach one structured field problem.
    pub fn with_field(mut self, problem: FieldProblem) -> Self {
        self.fields.push(problem);
        self
    }

    /// Attach one extension member.
    ///
    /// Extensions are carried verbatim; the rendering layer decides what is
    /// written on the wire. Applications must not use the framework's reserved
    /// `x-webx-*` keys.
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }

    /// The error category.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The stable machine-readable code (never empty).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The client-visible message, if one was supplied.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The client-visible message, if one was supplied.
    ///
    /// Alias of [`detail`](AppError::detail) under the name existing rendering
    /// code uses.
    pub fn public_detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The log-only diagnostic message, if one was supplied.
    pub fn operator(&self) -> Option<&str> {
        self.operator.as_deref()
    }

    /// Whether retrying the same request could succeed.
    pub fn retryable(&self) -> bool {
        self.retryable
    }

    /// The effective HTTP status: the explicit override, else the kind default.
    pub fn status_code(&self) -> u16 {
        self.status.unwrap_or_else(|| self.kind.default_status())
    }

    /// The structured field problems, in insertion order.
    pub fn fields(&self) -> &[FieldProblem] {
        &self.fields
    }

    /// Free-form extension members carried with the error.
    pub fn extensions(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.extensions
    }

    /// Mutable access to the extension members.
    ///
    /// Frameworks use this to move a batch of members (for example the
    /// extension members of a problem document) into the carrier in one go.
    pub fn extensions_mut(&mut self) -> &mut serde_json::Map<String, serde_json::Value> {
        &mut self.extensions
    }
}

/// A request body that is not valid JSON is a client error (`400
/// invalid_json`), never a server defect.
///
/// The opposite direction (a response DTO that cannot be serialized) has **no**
/// `From` impl on purpose: it must be built explicitly with
/// [`AppError::response_serialization`] so it maps to `500`.
impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::invalid_json(error)
    }
}

/// `Display` is written for operators, not for the wire: the log-only
/// `operator` message when present, else the client-visible `detail`, else the
/// stable code.
///
/// The rendering layer must use [`AppError::public_detail`] (or the status
/// title) for client output, never `Display`.
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.operator.as_deref(), self.detail.as_deref()) {
            (Some(operator), _) => f.write_str(operator),
            (None, Some(detail)) => f.write_str(detail),
            (None, None) => f.write_str(&self.code),
        }
    }
}

/// There is no `source()` chain: the carrier is a flat value. Framework
/// diagnostic types keep their own sources until they are converted.
impl std::error::Error for AppError {}

/// Shorthand for `Result<T, AppError>` — the error type of application handlers
/// and framework boundaries.
pub type Result<T> = std::result::Result<T, AppError>;
