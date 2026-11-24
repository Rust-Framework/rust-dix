//! `RdiError → rust_core::AppError` conversion (P1 unified error carrier).
//!
//! A DI failure is a composition defect: the shared carrier must expose it as a
//! hidden `500 dependency_injection` and keep the container message only in the
//! operator detail. No container internals may reach a client response.

use rust_core::{AppError, ErrorKind};
use rust_dix::RdiError;

#[test]
fn service_not_found_becomes_a_hidden_di_error() {
    let error: AppError =
        RdiError::not_found("OrderService", Some("OrderHandler".to_string())).into();

    assert_eq!(error.kind(), ErrorKind::Di);
    assert_eq!(error.status_code(), 500);
    assert_eq!(error.code(), "dependency_injection");
    assert_eq!(error.public_detail(), None);
    assert!(!error.retryable());
    let operator = error.operator().expect("operator detail");
    assert!(operator.contains("OrderService"), "{operator}");
    assert!(operator.contains("OrderHandler"), "{operator}");
}

#[test]
fn keyed_and_scope_violations_keep_the_container_message_operator_only() {
    for error in [
        RdiError::keyed_not_found("IClock", "read", None),
        RdiError::scoped_from_root("DbContext"),
        RdiError::ProviderDisposed,
    ] {
        let expected = error.to_string();
        let mapped: AppError = error.into();
        assert_eq!(mapped.kind(), ErrorKind::Di);
        assert_eq!(mapped.status_code(), 500);
        assert_eq!(
            mapped.public_detail(),
            None,
            "container internals must never reach the wire"
        );
        assert_eq!(mapped.operator(), Some(expected.as_str()));
    }
}
