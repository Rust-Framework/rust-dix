//! Contract tests for the shared application-error carrier.
//!
//! These tests pin the behaviours the three frameworks rely on when they
//! convert their diagnostics into [`AppError`]: classification, wire code,
//! retryability, client/operator separation, field problems, extensions and the
//! stable error-code registry.

use rust_core::error_codes::{
    default_code_for_status, status_is_retryable, ERROR_CODES, FALLBACK_STATUS,
};
use rust_core::{AppError, ErrorKind, FieldProblem};

/// Every `ErrorKind` with its expected default mapping.
const KIND_DEFAULTS: &[(ErrorKind, u16, &str, bool)] = &[
    (ErrorKind::InvalidInput, 400, "bad_request", false),
    (ErrorKind::Unauthorized, 401, "unauthorized", false),
    (ErrorKind::Forbidden, 403, "forbidden", false),
    (ErrorKind::NotFound, 404, "not_found", false),
    (ErrorKind::Conflict, 409, "conflict", false),
    (ErrorKind::PayloadTooLarge, 413, "payload_too_large", false),
    (
        ErrorKind::UnsupportedMediaType,
        415,
        "unsupported_media_type",
        false,
    ),
    (ErrorKind::RateLimited, 429, "too_many_requests", true),
    (ErrorKind::Unavailable, 503, "service_unavailable", true),
    (ErrorKind::Internal, 500, "internal_error", false),
    (ErrorKind::Di, 500, "dependency_injection", false),
    (ErrorKind::Other, 500, "error", false),
];

fn serde_error() -> serde_json::Error {
    serde_json::from_str::<serde_json::Value>("{").expect_err("malformed JSON must fail")
}

fn assert_send_sync_static<T: Send + Sync + 'static>() {}

#[test]
fn every_error_kind_has_a_stable_default_mapping() {
    assert_eq!(KIND_DEFAULTS.len(), 12, "all kinds are covered");

    for (kind, status, code, retryable) in KIND_DEFAULTS {
        let error = AppError::new(*kind, *code);
        assert_eq!(error.kind(), *kind);
        assert_eq!(error.status_code(), *status, "status of {kind:?}");
        assert_eq!(error.code(), *code, "code of {kind:?}");
        assert_eq!(error.retryable(), *retryable, "retryable of {kind:?}");
        assert_eq!(kind.default_status(), *status, "default_status of {kind:?}");
        assert_eq!(kind.default_code(), *code, "default_code of {kind:?}");
        assert_eq!(
            kind.default_retryable(),
            *retryable,
            "default_retryable of {kind:?}"
        );
    }
}

#[test]
fn from_status_covers_every_family() {
    let cases = [
        (400, ErrorKind::InvalidInput),
        (401, ErrorKind::Unauthorized),
        (403, ErrorKind::Forbidden),
        (404, ErrorKind::NotFound),
        (405, ErrorKind::Other),
        (409, ErrorKind::Conflict),
        (413, ErrorKind::PayloadTooLarge),
        (415, ErrorKind::UnsupportedMediaType),
        (418, ErrorKind::Other),
        (422, ErrorKind::Other),
        (429, ErrorKind::RateLimited),
        (500, ErrorKind::Internal),
        (501, ErrorKind::Other),
        (502, ErrorKind::Unavailable),
        (503, ErrorKind::Unavailable),
        (504, ErrorKind::Unavailable),
        (599, ErrorKind::Internal),
    ];
    for (status, kind) in cases {
        assert_eq!(ErrorKind::from_status(status), kind, "status {status}");
    }
}

#[test]
fn client_and_operator_text_stay_separate() {
    let validation = AppError::validation("email is invalid");
    assert_eq!(validation.kind(), ErrorKind::InvalidInput);
    assert_eq!(validation.status_code(), 400);
    assert_eq!(validation.code(), "validation_failed");
    assert_eq!(validation.public_detail(), Some("email is invalid"));
    assert_eq!(validation.operator(), None);
    assert!(!validation.retryable());

    let internal = AppError::internal("db pool exhausted");
    assert_eq!(internal.kind(), ErrorKind::Internal);
    assert_eq!(internal.status_code(), 500);
    assert_eq!(internal.code(), "internal_error");
    assert_eq!(internal.public_detail(), None);
    assert_eq!(internal.operator(), Some("db pool exhausted"));
    assert!(!internal.retryable());

    let di = AppError::di("OrderService is not registered");
    assert_eq!(di.kind(), ErrorKind::Di);
    assert_eq!(di.status_code(), 500);
    assert_eq!(di.code(), "dependency_injection");
    assert_eq!(di.public_detail(), None);
    assert_eq!(di.operator(), Some("OrderService is not registered"));

    let state = AppError::state("response body already written");
    assert_eq!(state.code(), "invalid_state");
    assert_eq!(state.status_code(), 500);
    assert_eq!(state.public_detail(), None);
    assert_eq!(state.operator(), Some("response body already written"));
}

#[test]
fn every_semantic_constructor_maps_to_its_documented_shape() {
    let cases: &[(AppError, ErrorKind, u16, &str, Option<&str>)] = &[
        (
            AppError::bad_request("bad"),
            ErrorKind::InvalidInput,
            400,
            "bad_request",
            Some("bad"),
        ),
        (
            AppError::validation("bad"),
            ErrorKind::InvalidInput,
            400,
            "validation_failed",
            Some("bad"),
        ),
        (
            AppError::unauthorized("auth"),
            ErrorKind::Unauthorized,
            401,
            "unauthorized",
            Some("auth"),
        ),
        (
            AppError::forbidden("deny"),
            ErrorKind::Forbidden,
            403,
            "forbidden",
            Some("deny"),
        ),
        (
            AppError::not_found("missing"),
            ErrorKind::NotFound,
            404,
            "not_found",
            Some("missing"),
        ),
        (
            AppError::route_not_found("no route"),
            ErrorKind::NotFound,
            404,
            "route_not_found",
            Some("no route"),
        ),
        (
            AppError::conflict("version"),
            ErrorKind::Conflict,
            409,
            "conflict",
            Some("version"),
        ),
        (
            AppError::conflict_retryable("in flight"),
            ErrorKind::Conflict,
            409,
            "conflict_retryable",
            Some("in flight"),
        ),
        (
            AppError::payload_too_large("too big"),
            ErrorKind::PayloadTooLarge,
            413,
            "payload_too_large",
            Some("too big"),
        ),
        (
            AppError::unsupported_media_type("xml"),
            ErrorKind::UnsupportedMediaType,
            415,
            "unsupported_media_type",
            Some("xml"),
        ),
        (
            AppError::too_many_requests(7, "slow down"),
            ErrorKind::RateLimited,
            429,
            "too_many_requests",
            Some("slow down"),
        ),
        (
            AppError::unavailable("maintenance"),
            ErrorKind::Unavailable,
            503,
            "service_unavailable",
            Some("maintenance"),
        ),
    ];

    for (error, kind, status, code, detail) in cases {
        assert_eq!(error.kind(), *kind, "kind of {code}");
        assert_eq!(error.status_code(), *status, "status of {code}");
        assert_eq!(error.code(), *code, "code of {code}");
        assert_eq!(error.public_detail(), *detail, "detail of {code}");
    }

    // Retryability is explicit where the kind default does not fit.
    assert!(AppError::conflict_retryable("in flight").retryable());
    assert!(!AppError::conflict("version").retryable());
    assert!(AppError::too_many_requests(7, "slow down").retryable());
    assert!(AppError::unavailable("maintenance").retryable());
}

#[test]
fn status_escape_hatch_derives_kind_and_code() {
    let teapot = AppError::status(418, "short and stout");
    assert_eq!(teapot.kind(), ErrorKind::Other);
    assert_eq!(teapot.status_code(), 418);
    assert_eq!(teapot.code(), "error");
    assert_eq!(teapot.public_detail(), Some("short and stout"));
    assert!(!teapot.retryable());

    let unprocessable = AppError::status(422, "invalid entity");
    assert_eq!(unprocessable.kind(), ErrorKind::Other);
    assert_eq!(unprocessable.code(), "unprocessable_entity");
    assert_eq!(unprocessable.status_code(), 422);

    let not_implemented = AppError::status(501, "not yet");
    assert_eq!(not_implemented.kind(), ErrorKind::Other);
    assert_eq!(not_implemented.code(), "not_implemented");

    let unavailable = AppError::status(503, "down");
    assert_eq!(unavailable.kind(), ErrorKind::Unavailable);
    assert!(unavailable.retryable());

    // Explicit builders override the derived defaults.
    let overridden = AppError::status(503, "down")
        .with_status(504)
        .with_retryable(false);
    assert_eq!(overridden.status_code(), 504);
    assert!(!overridden.retryable());
}

#[test]
fn empty_codes_are_replaced_by_the_kind_default() {
    let error = AppError::new(ErrorKind::NotFound, "");
    assert_eq!(error.code(), "not_found");
    assert!(!error.code().is_empty());
}

#[test]
fn request_json_errors_are_client_errors() {
    let error: AppError = serde_error().into();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(error.status_code(), 400);
    assert_eq!(error.code(), "invalid_json");
    assert_eq!(error.public_detail(), None);
    assert!(
        error.operator().is_some_and(|text| !text.is_empty()),
        "the serde message must be preserved for operators"
    );

    // The response-side defect has no `From`; it is built explicitly and maps
    // to 500 with the serde text in `operator`.
    let response = AppError::response_serialization(serde_error());
    assert_eq!(response.kind(), ErrorKind::Internal);
    assert_eq!(response.status_code(), 500);
    assert_eq!(response.code(), "response_serialization");
    assert_eq!(response.public_detail(), None);
    assert!(response.operator().is_some());

    // The two paths must stay distinguishable.
    assert_ne!(error.status_code(), response.status_code());
    assert_ne!(error.code(), response.code());
}

#[test]
fn field_problems_keep_order_and_metadata() {
    let problem = FieldProblem::new("email", "invalid format").with_code("invalid_format");
    assert_eq!(problem.field, "email");
    assert_eq!(problem.message, "invalid format");
    assert_eq!(problem.code.as_deref(), Some("invalid_format"));

    let plain = FieldProblem::new("page", "must be a positive integer");
    assert_eq!(plain.code, None);

    let error = AppError::validation("one or more fields are invalid")
        .with_field(problem)
        .with_field(plain);
    assert_eq!(error.fields().len(), 2);
    assert_eq!(error.fields()[0].field, "email");
    assert_eq!(error.fields()[1].field, "page");
    assert_eq!(error.fields()[0].code.as_deref(), Some("invalid_format"));
}

#[test]
fn extensions_roundtrip_through_json() {
    let error = AppError::internal("boom")
        .with_extension("request_id", "req-7")
        .with_extension("attempts", 3_u64)
        .with_extension("fatal", true);
    assert_eq!(
        error.extensions().get("request_id"),
        Some(&serde_json::json!("req-7"))
    );
    assert_eq!(
        error.extensions().get("attempts"),
        Some(&serde_json::json!(3))
    );
    assert_eq!(
        error.extensions().get("fatal"),
        Some(&serde_json::json!(true))
    );

    let encoded = serde_json::to_string(error.extensions()).expect("extensions serialize");
    let decoded: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&encoded).expect("extensions deserialize");
    assert_eq!(&decoded, error.extensions());

    let mut error = error;
    error.extensions_mut().remove("fatal");
    assert!(error.extensions().get("fatal").is_none());
    assert_eq!(error.extensions().len(), 2);
}

#[test]
fn typed_rate_limit_carries_its_retry_after_value() {
    let limited = AppError::too_many_requests(7, "quota exhausted");
    assert_eq!(limited.status_code(), 429);
    assert_eq!(limited.code(), "too_many_requests");
    assert!(limited.retryable());
    assert_eq!(limited.public_detail(), Some("quota exhausted"));
    assert_eq!(
        limited.extensions().get("x-webx-retry-after"),
        Some(&serde_json::json!(7)),
        "the wait value must survive the trip from handler to response layer"
    );

    // A bare 429 has no wait value.
    let bare = AppError::status(429, "slow down");
    assert!(bare.retryable());
    assert!(bare.extensions().get("x-webx-retry-after").is_none());

    // A retryable conflict carries no wait value either.
    assert!(AppError::conflict_retryable("in flight")
        .extensions()
        .get("x-webx-retry-after")
        .is_none());

    // An explicit override wins over the kind default.
    assert!(!AppError::too_many_requests(7, "slow down")
        .with_retryable(false)
        .retryable());
}

#[test]
fn display_prefers_operator_then_detail_then_code() {
    assert_eq!(AppError::internal("db down").to_string(), "db down");
    assert_eq!(
        AppError::not_found("order '42' missing").to_string(),
        "order '42' missing"
    );

    let bare = AppError::new(ErrorKind::Forbidden, "forbidden");
    assert_eq!(bare.to_string(), "forbidden");

    let both = AppError::not_found("client text").with_operator("log text");
    assert_eq!(both.to_string(), "log text");
}

#[test]
fn the_error_type_is_a_send_sync_static_value_without_a_source_chain() {
    assert_send_sync_static::<AppError>();
    assert_send_sync_static::<FieldProblem>();
    assert_send_sync_static::<ErrorKind>();

    let error = AppError::response_serialization(serde_error());
    assert!(!error.to_string().is_empty());
    let as_std: &dyn std::error::Error = &error;
    assert!(
        as_std.source().is_none(),
        "the carrier is flat; sources stay in the framework diagnostic types"
    );
}

#[test]
fn error_code_registry_is_unique_consistent_and_complete() {
    assert_eq!(
        ERROR_CODES.len(),
        23,
        "registry size is part of the contract"
    );

    // One entry per code, and one status per code.
    for (index, spec) in ERROR_CODES.iter().enumerate() {
        assert!(
            ERROR_CODES
                .iter()
                .skip(index + 1)
                .all(|other| other.code != spec.code),
            "duplicate code {}",
            spec.code
        );
    }

    // `retryable` agrees with the status rule, except the documented
    // condition-specific `conflict_retryable`.
    for spec in ERROR_CODES {
        if spec.code == "conflict_retryable" {
            assert_eq!(spec.status, 409);
            assert!(spec.retryable);
            continue;
        }
        assert_eq!(
            spec.retryable,
            status_is_retryable(spec.status),
            "retryable of {}",
            spec.code
        );
    }

    // Every code reachable through the runtime functions is registered with the
    // same status/retryability.
    for (_, status, code, retryable) in KIND_DEFAULTS {
        let spec = ERROR_CODES
            .iter()
            .find(|spec| spec.code == *code)
            .unwrap_or_else(|| panic!("{code} must be registered"));
        let expected_status = if spec.status == FALLBACK_STATUS {
            *status
        } else {
            spec.status
        };
        assert_eq!(expected_status, *status, "status of {code}");
        assert_eq!(spec.retryable, *retryable, "retryable of {code}");
    }

    for status in [
        400, 401, 403, 404, 405, 409, 413, 415, 422, 429, 500, 501, 502, 503, 504,
    ] {
        let code = default_code_for_status(status);
        assert!(
            ERROR_CODES.iter().any(|spec| spec.code == code),
            "default code {code} (status {status}) must be registered"
        );
    }
    assert_eq!(default_code_for_status(599), "error");
    assert_eq!(
        ERROR_CODES
            .iter()
            .find(|spec| spec.code == "error")
            .map(|spec| spec.status),
        Some(FALLBACK_STATUS)
    );
}

#[test]
fn semantic_constructors_only_emit_registered_codes() {
    let constructors: &[AppError] = &[
        AppError::bad_request("x"),
        AppError::validation("x"),
        AppError::unauthorized("x"),
        AppError::forbidden("x"),
        AppError::not_found("x"),
        AppError::route_not_found("x"),
        AppError::conflict("x"),
        AppError::conflict_retryable("x"),
        AppError::payload_too_large("x"),
        AppError::unsupported_media_type("x"),
        AppError::too_many_requests(3, "x"),
        AppError::unavailable("x"),
        AppError::internal("x"),
        AppError::di("x"),
        AppError::state("x"),
        AppError::response_serialization(serde_error()),
        AppError::invalid_json(serde_error()),
    ];

    for error in constructors {
        let spec = ERROR_CODES
            .iter()
            .find(|spec| spec.code == error.code())
            .unwrap_or_else(|| panic!("unregistered code {}", error.code()));
        assert_eq!(
            spec.status,
            error.status_code(),
            "status of {}",
            error.code()
        );
        assert_eq!(
            spec.retryable,
            error.retryable(),
            "retryable of {}",
            error.code()
        );
    }
}
