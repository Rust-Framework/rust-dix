//! Contract tests for the offset-pagination types.

use rust_core::pagination::{PagedRequest, PagedResult};
use rust_core::{DEFAULT_PAGE, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};

/// A domain-ish row that deliberately does **not** implement `Serialize`.
struct Entity {
    id: u32,
}

#[test]
fn paged_request_carries_no_normalization_state() {
    let paging = PagedRequest {
        page: 3,
        page_size: 50,
    };
    assert_eq!(paging.page, 3);
    assert_eq!(paging.page_size, 50);
    assert_eq!(paging.limit(), 50);

    let defaults = PagedRequest {
        page: DEFAULT_PAGE,
        page_size: DEFAULT_PAGE_SIZE,
    };
    assert_eq!((defaults.page, defaults.page_size), (1, 20));
    assert_eq!(MAX_PAGE_SIZE, 100);

    // The raw value is preserved: no clamping happens in the contract type.
    let raw = PagedRequest {
        page: 0,
        page_size: 1_000,
    };
    assert_eq!((raw.page, raw.page_size), (0, 1_000));
    assert_eq!(raw.limit(), 1_000);
}

#[test]
fn offset_is_u64_and_cannot_overflow() {
    assert_eq!(
        PagedRequest {
            page: 1,
            page_size: 20
        }
        .offset(),
        0
    );
    assert_eq!(
        PagedRequest {
            page: 3,
            page_size: 100
        }
        .offset(),
        200
    );

    // `u32::MAX` rows per page starting at the last page: the product is still
    // representable in `u64`, so the offset must be exact rather than saturated.
    let hostile = PagedRequest {
        page: u32::MAX,
        page_size: u32::MAX,
    };
    assert_eq!(
        hostile.offset(),
        u64::from(u32::MAX - 1) * u64::from(u32::MAX)
    );
    assert!(hostile.offset() < u64::MAX);

    // Page 0 is not addressable; it saturates to the first row instead of
    // underflowing.
    assert_eq!(
        PagedRequest {
            page: 0,
            page_size: 20
        }
        .offset(),
        0
    );
    assert_eq!(
        PagedRequest {
            page: 5,
            page_size: 0
        }
        .offset(),
        0
    );
}

#[test]
fn paged_result_serializes_as_one_flat_operation_envelope() {
    let page = PagedResult::new(vec!["a", "b"], 7);
    let value = serde_json::to_value(&page).expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({
            "success": true,
            "status": 0,
            "message": "ok",
            "items": ["a", "b"],
            "total": 7
        })
    );

    // The page part carries no request echo; the client computes those from the
    // request it sent. The operation fields are the shared envelope.
    let object = value.as_object().expect("object");
    assert_eq!(object.len(), 5);
    assert!(!object.contains_key("page"));
    assert!(!object.contains_key("page_size"));
    assert!(!object.contains_key("total_pages"));
}

#[test]
fn paged_result_can_override_the_operation_outcome() {
    let page = PagedResult::new(vec![1, 2], 2)
        .with_status(1001)
        .with_message("already listed");
    assert!(page.success());
    assert_eq!(page.status(), 1001);
    assert_eq!(page.message(), "already listed");
    assert_eq!(page.operation().status, 1001);

    let failed = PagedResult::<u32>::failed(4004, "filter rejected");
    assert!(!failed.success());
    assert!(failed.items.is_empty());
    assert_eq!(failed.total, 0);
    let value = serde_json::to_value(&failed).expect("serialize");
    assert_eq!(value["success"], false);
    assert_eq!(value["status"], 4004);
    assert_eq!(value["message"], "filter rejected");
    assert_eq!(value["items"], serde_json::json!([]));
}

#[test]
fn paged_result_has_no_serialize_bound_on_entities() {
    let page: PagedResult<Entity> = PagedResult::new(vec![Entity { id: 1 }, Entity { id: 2 }], 9);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.total, 9);

    let mapped = page.map_items(|entity| format!("row-{}", entity.id));
    assert_eq!(mapped.items, vec!["row-1".to_string(), "row-2".to_string()]);
    assert_eq!(mapped.total, 9, "map_items keeps the total");
    assert!(mapped.success(), "map_items preserves the outcome");

    let value = serde_json::to_value(&mapped).expect("serialize mapped page");
    assert_eq!(
        value,
        serde_json::json!({
            "success": true,
            "status": 0,
            "message": "ok",
            "items": ["row-1", "row-2"],
            "total": 9
        })
    );
}

#[test]
fn paged_request_json_requires_the_fields_and_rejects_negatives() {
    // Defaults and range validation belong to the binding layer, not to the
    // contract type: a bare JSON object with no fields is not a valid request.
    assert!(serde_json::from_str::<PagedRequest>(r#"{}"#).is_err());
    assert!(serde_json::from_str::<PagedRequest>(r#"{ "page": 1 }"#).is_err());

    let parsed: PagedRequest =
        serde_json::from_str(r#"{ "page": 2, "page_size": 50 }"#).expect("valid request");
    assert_eq!((parsed.page, parsed.page_size), (2, 50));

    // `u32` is the contract: negatives never reach the handler.
    assert!(serde_json::from_str::<PagedRequest>(r#"{ "page": -1, "page_size": 20 }"#).is_err());
    assert!(serde_json::from_str::<PagedRequest>(r#"{ "page": 1, "page_size": -20 }"#).is_err());
}
