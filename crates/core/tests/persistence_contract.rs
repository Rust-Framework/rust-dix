//! Contract tests for the incremental-update (patch) vocabulary.

use rust_core::IPatchable;

#[derive(Debug, Clone, PartialEq)]
struct Order {
    title: String,
    quantity: u32,
}

/// Full partial update: every member is optional, absent means "leave it".
struct UpdateOrderRequest {
    title: Option<String>,
    quantity: Option<u32>,
}

/// A different patch shape for the same entity: cancelling releases the
/// quantity, so the entity has two independent `IPatchable` implementations.
struct CancelOrderRequest;

impl IPatchable<UpdateOrderRequest> for Order {
    fn patch(&mut self, patch: UpdateOrderRequest) {
        if let Some(title) = patch.title {
            self.title = title;
        }
        if let Some(quantity) = patch.quantity {
            self.quantity = quantity;
        }
    }
}

impl IPatchable<CancelOrderRequest> for Order {
    fn patch(&mut self, _patch: CancelOrderRequest) {
        // Cancelling only touches the quantity; the title stays as it was.
        self.quantity = 0;
    }
}

/// The entity is usable behind the contract alone, without knowing the patch
/// type at the call site.
fn apply<Patch, Entity: IPatchable<Patch>>(entity: &mut Entity, patch: Patch) {
    entity.patch(patch);
}

fn sample_order() -> Order {
    Order {
        title: "book".into(),
        quantity: 3,
    }
}

#[test]
fn a_patch_overwrites_only_the_fields_it_carries() {
    let mut order = sample_order();
    order.patch(UpdateOrderRequest {
        title: Some("pen".into()),
        quantity: None,
    });
    assert_eq!(order.title, "pen");
    assert_eq!(order.quantity, 3, "quantity was absent, so it is untouched");

    order.patch(UpdateOrderRequest {
        title: None,
        quantity: Some(7),
    });
    assert_eq!(order.title, "pen", "title was absent, so it is untouched");
    assert_eq!(order.quantity, 7);
}

#[test]
fn a_patch_with_no_fields_leaves_the_entity_unchanged() {
    let mut order = sample_order();
    order.patch(UpdateOrderRequest {
        title: None,
        quantity: None,
    });
    assert_eq!(order, sample_order());
}

#[test]
fn the_same_entity_can_be_patched_by_two_request_types() {
    let mut order = sample_order();
    apply(
        &mut order,
        UpdateOrderRequest {
            title: Some("pen".into()),
            quantity: Some(7),
        },
    );
    assert_eq!(
        order,
        Order {
            title: "pen".into(),
            quantity: 7
        }
    );

    apply(&mut order, CancelOrderRequest);
    assert_eq!(
        order,
        Order {
            title: "pen".into(),
            quantity: 0
        },
        "CancelOrderRequest is a second patch type on the same entity"
    );
}
