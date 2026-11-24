//! Contract tests for the keyset (cursor) pagination types.

use rust_core::keyset::{Cursor, CursorError, CursorPage, KeysetRequest, MAX_CURSOR_BYTES};
use rust_core::{AppError, ErrorKind};

/// A domain entity that deliberately does **not** implement `Serialize`.
struct Message {
    created_at: i64,
    id: &'static str,
}

#[derive(serde::Serialize, PartialEq, Debug)]
struct MessageDto {
    id: &'static str,
}

#[test]
fn cursor_round_trips_sort_components() {
    let components = ["2026-11-24T12:00:00Z", "订单-42", "x"];
    let cursor = Cursor::encode(&components);
    assert_eq!(
        Cursor::decode(&cursor).expect("decode"),
        components.map(str::to_string)
    );

    // Whitespace around the opaque value is tolerated (query strings carry it).
    assert_eq!(
        Cursor::decode(&format!("  {cursor}  ")).expect("decode trimmed"),
        components.map(str::to_string)
    );

    // An empty component list is a valid cursor (first-page marker).
    let empty = Cursor::encode(&[]);
    assert!(Cursor::decode(&empty).expect("decode empty").is_empty());
}

#[test]
fn malformed_cursors_fail_with_typed_errors() {
    assert_eq!(Cursor::decode("not base64!!"), Err(CursorError::Encoding));
    // Valid base64, non-canonical trailing bits.
    assert_eq!(Cursor::decode("Zm9"), Err(CursorError::Encoding));

    // Valid base64url whose payload is not a JSON array of strings.
    let numbers = "WzEsMl0"; // base64url("[1,2]")
    assert_eq!(Cursor::decode(numbers), Err(CursorError::Format));
    let object_json = "eyJhIjoxfQ"; // base64url("{\"a\":1}")
    assert_eq!(Cursor::decode(object_json), Err(CursorError::Format));
}

#[test]
fn cursor_length_limit_is_exact() {
    // Exactly the limit is accepted (and then fails later because it is not a
    // JSON string array); one byte over is rejected early.
    let at_limit = "a".repeat(MAX_CURSOR_BYTES);
    assert_eq!(Cursor::decode(&at_limit), Err(CursorError::Format));
    let over_limit = "a".repeat(MAX_CURSOR_BYTES + 1);
    assert_eq!(Cursor::decode(&over_limit), Err(CursorError::TooLong));
}

#[test]
fn cursor_errors_become_a_sanitized_400() {
    let error: AppError = CursorError::Format.into();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(error.status_code(), 400);
    assert_eq!(error.code(), "validation_failed");
    assert!(!error.retryable());
    assert_eq!(
        error.public_detail(),
        Some("cursor is not a cursor for this endpoint")
    );
    assert_eq!(error.operator(), None);

    // The raw client value is never echoed back into the response text.
    let raw = "super-secret-cursor-payload!!";
    let error: AppError = Cursor::decode(raw).unwrap_err().into();
    assert!(
        !error.public_detail().unwrap_or_default().contains(raw),
        "the cursor value must not be reflected to the client"
    );
}

#[test]
fn keyset_request_uses_serde_defaults_for_the_page_size() {
    let request: KeysetRequest = serde_json::from_str("{}").expect("defaults");
    assert_eq!(request.cursor, None);
    assert_eq!(request.page_size, 20);
    assert_eq!(request.cursor(), None);
    assert_eq!(request.page_size(), 20);

    let request: KeysetRequest =
        serde_json::from_str(r#"{ "cursor": "abc", "page_size": 0 }"#).expect("parsed");
    assert_eq!(request.cursor.as_deref(), Some("abc"));
    assert_eq!(request.page_size(), 1, "page_size() clamps a raw zero");
}

#[test]
fn keyset_request_constructors_default_and_clamp() {
    let defaults = KeysetRequest::from_parts(None, None);
    assert_eq!(defaults.cursor, None);
    assert_eq!(defaults.page_size, 20);

    let zero = KeysetRequest::from_parts(Some("c".into()), Some(0));
    assert_eq!(zero.page_size, 1);
    assert_eq!(zero.cursor.as_deref(), Some("c"));

    let wide = KeysetRequest::from_parts(None, Some(1_000));
    assert_eq!(wide.page_size, 100);

    assert_eq!(KeysetRequest::new(None, 50).page_size(), 50);
    assert_eq!(
        KeysetRequest {
            cursor: None,
            page_size: 1_000
        }
        .clamped()
        .page_size,
        100
    );
}

#[test]
fn request_decoded_treats_absent_and_empty_as_first_page() {
    assert!(KeysetRequest::from_parts(None, None)
        .decoded()
        .expect("none")
        .is_empty());
    assert!(KeysetRequest::from_parts(Some(String::new()), None)
        .decoded()
        .expect("empty")
        .is_empty());
    assert!(KeysetRequest::from_parts(Some("bad!".into()), None)
        .decoded()
        .is_err());
}

#[test]
fn pages_expose_an_exact_has_more_from_one_extra_row() {
    struct Row {
        id: &'static str,
    }

    let rows = vec![Row { id: "a" }, Row { id: "b" }, Row { id: "c" }];

    // page_size 2 with 3 fetched rows: two items, a cursor, has_more.
    let page = CursorPage::from_rows(rows, 2, |row| vec![row.id.to_string()]);
    assert_eq!(page.items.len(), 2);
    assert!(page.has_more);
    let cursor = page.cursor.clone().expect("cursor");
    assert_eq!(
        Cursor::decode(&cursor).expect("decode"),
        vec!["b".to_string()]
    );

    // Exactly page_size rows: no cursor, no more pages.
    let rows = vec![Row { id: "a" }, Row { id: "b" }];
    let page = CursorPage::from_rows(rows, 2, |row| vec![row.id.to_string()]);
    assert_eq!(page.items.len(), 2);
    assert!(!page.has_more);
    assert!(page.cursor.is_none());
}

#[test]
fn from_entities_maps_only_kept_rows_and_never_needs_entity_serialize() {
    let rows = vec![
        Message {
            created_at: 0,
            id: "a",
        },
        Message {
            created_at: 1,
            id: "b",
        },
        Message {
            created_at: 2,
            id: "c",
        },
    ];
    let mut mapped = 0usize;

    let page: CursorPage<MessageDto> = CursorPage::from_entities(
        rows,
        2,
        |row| vec![row.created_at.to_string(), row.id.to_string()],
        |row| {
            mapped += 1;
            MessageDto { id: row.id }
        },
    );

    assert_eq!(
        page.items,
        vec![MessageDto { id: "a" }, MessageDto { id: "b" }]
    );
    // The probe row is dropped before mapping.
    assert_eq!(mapped, 2);
    assert!(page.has_more);
    assert_eq!(
        Cursor::decode(page.cursor.as_deref().expect("cursor")).expect("decode"),
        vec!["1".to_string(), "b".to_string()]
    );
}

#[test]
fn serialized_page_omits_an_absent_cursor() {
    let page: CursorPage<u32> = CursorPage::new(vec![1, 2], None);
    let json = serde_json::to_value(&page).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({ "items": [1, 2], "has_more": false })
    );
    assert!(json.get("cursor").is_none());

    let page: CursorPage<u32> = CursorPage::new(vec![1], Some("next".into()));
    let json = serde_json::to_value(&page).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({ "items": [1], "cursor": "next", "has_more": true })
    );
}

#[test]
fn map_items_keeps_the_page_envelope() {
    let page: CursorPage<u32> = CursorPage::new(vec![1, 2], Some("next".to_string()));
    let mapped = page.map_items(|id| MessageDto {
        id: if id == 1 { "a" } else { "b" },
    });
    assert_eq!(
        mapped.items,
        vec![MessageDto { id: "a" }, MessageDto { id: "b" }],
        "items are transformed in order"
    );
    assert_eq!(mapped.cursor.as_deref(), Some("next"));
    assert!(mapped.has_more, "has_more must not be recomputed/lost");

    let last: CursorPage<u32> = CursorPage::new(vec![3], None);
    let mapped = last.map_items(|id| MessageDto {
        id: if id == 3 { "c" } else { "?" },
    });
    assert!(!mapped.has_more);
    assert!(mapped.cursor.is_none());
}
