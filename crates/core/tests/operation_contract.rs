//! Contract tests for the business result envelope.

use rust_core::OperationResult;

#[derive(serde::Serialize, PartialEq, Debug)]
struct UserDto {
    id: u32,
}

#[test]
fn unit_result_omits_the_result_member() {
    let value = serde_json::to_value(OperationResult::success("deleted")).expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({ "success": true, "status": 0, "message": "deleted" })
    );
}

#[test]
fn generic_result_carries_the_payload_and_its_business_status() {
    let value = serde_json::to_value(OperationResult::ok_with_status(
        7,
        UserDto { id: 7 },
        "created",
    ))
    .expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({
            "success": true,
            "status": 7,
            "message": "created",
            "result": { "id": 7 }
        })
    );

    let default_status =
        serde_json::to_value(OperationResult::ok(UserDto { id: 1 }, "ok")).expect("serialize");
    assert_eq!(default_status["status"], 0);
}

#[test]
fn failed_result_keeps_the_business_status_and_drops_the_payload() {
    let value = serde_json::to_value(OperationResult::<UserDto>::failed(1001, "state conflict"))
        .expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({
            "success": false,
            "status": 1001,
            "message": "state conflict"
        })
    );
    assert!(value.get("result").is_none());
}

#[test]
fn setters_adjust_an_outcome_decided_while_the_operation_runs() {
    let mut outcome = OperationResult::success_with_status(3, "published");
    outcome = outcome.with_status(1001).with_message("already published");
    // The fields stay public for in-place adjustment too.
    outcome.status = 1001;

    let value = serde_json::to_value(outcome).expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({
            "success": true,
            "status": 1001,
            "message": "already published"
        })
    );
}

#[test]
fn with_result_attaches_the_payload_to_a_unit_envelope() {
    let value =
        serde_json::to_value(OperationResult::success("created").with_result(UserDto { id: 9 }))
            .expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({
            "success": true,
            "status": 0,
            "message": "created",
            "result": { "id": 9 }
        })
    );
}

#[test]
fn envelope_round_trips_through_json() {
    let outcome = OperationResult::ok_with_status(7, UserDto { id: 7 }, "created");
    let encoded = serde_json::to_string(&outcome).expect("serialize");
    let decoded: serde_json::Value = serde_json::from_str(&encoded).expect("deserialize value");

    assert_eq!(decoded["success"], true);
    assert_eq!(decoded["status"], 7);
    assert_eq!(decoded["message"], "created");
    assert_eq!(decoded["result"]["id"], 7);

    let reencoded = serde_json::to_string(&decoded).expect("re-serialize");
    let reparsed: serde_json::Value = serde_json::from_str(&reencoded).expect("re-parse");
    assert_eq!(reparsed, decoded, "the envelope shape is stable");
}
