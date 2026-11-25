//! Persistence-facing contracts owned by the application.
//!
//! rust-core only defines the vocabulary; the entity, the DTOs and the mapping
//! bodies stay in the downstream business crate.
//!
//! [`IPatchable`] is the counterpart of a partial-update request:
//!
//! ```rust
//! # use rust_core::IPatchable;
//! # struct Order { title: String, quantity: u32 }
//! # struct UpdateOrderRequest { title: Option<String>, quantity: Option<u32> }
//! # impl UpdateOrderRequest {
//! #     fn apply_to(self, order: &mut Order) {
//! #         if let Some(title) = self.title { order.title = title; }
//! #         if let Some(quantity) = self.quantity { order.quantity = quantity; }
//! #     }
//! # }
//! impl IPatchable<UpdateOrderRequest> for Order {
//!     fn patch(&mut self, patch: UpdateOrderRequest) {
//!         patch.apply_to(self); // only the fields present in the patch are overwritten
//!     }
//! }
//! # fn main() {
//! #     let mut order = Order { title: "book".into(), quantity: 1 };
//! #     order.patch(UpdateOrderRequest { title: Some("pen".into()), quantity: None });
//! #     assert_eq!(order.title, "pen");
//! #     assert_eq!(order.quantity, 1, "an absent field is untouched");
//! # }
//! ```

/// An aggregate entity that can be incrementally updated by a patch request.
///
/// `TPatch` is the update request type (`UpdateOrderRequest`,
/// `CancelOrderRequest`, …). One entity may implement this several times; the
/// implementation must only overwrite the fields present in the patch and
/// leave the rest untouched.
pub trait IPatchable<TPatch> {
    /// Apply the patch onto `self`.
    fn patch(&mut self, patch: TPatch);
}
