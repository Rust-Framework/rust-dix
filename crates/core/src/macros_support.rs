//! Helpers invoked by `rust-dix-macros` generated code.
//!
//! These are `#[doc(hidden)]` — public so macro expansions in downstream crates
//! can call them, but not part of the supported application API.

use crate::entry::IServiceResolver;
use crate::error::RdiError;
use crate::provider::ServiceProvider;
use std::any::Any;
use std::sync::Arc;

fn field_context(consumer: &'static str, field: &'static str) -> String {
    format!("{consumer}::{field}")
}

/// Resolve `Arc<T>` for a required inject field.
#[doc(hidden)]
pub fn resolve_arc<T: ?Sized + Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    consumer: &'static str,
    field: &'static str,
) -> Result<Arc<T>, RdiError> {
    let ty = std::any::type_name::<T>();
    let ctx = field_context(consumer, field);
    resolver
        .get_any(ty)
        .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
        .ok_or_else(|| RdiError::not_found(ty, Some(ctx)))
}

/// Resolve keyed `Arc<T>` for a required inject field.
#[doc(hidden)]
pub fn resolve_keyed_arc<T: ?Sized + Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    key: &str,
    consumer: &'static str,
    field: &'static str,
) -> Result<Arc<T>, RdiError> {
    let ty = std::any::type_name::<T>();
    let ctx = field_context(consumer, field);
    resolver
        .get_keyed_any(ty, key)
        .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
        .ok_or_else(|| RdiError::keyed_not_found(ty, key, Some(ctx)))
}

/// Optional shared inject field.
#[doc(hidden)]
pub fn resolve_option_arc<T: ?Sized + Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
) -> Option<Arc<T>> {
    let ty = std::any::type_name::<T>();
    resolver
        .get_any(ty)
        .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
}

/// Optional keyed shared inject field.
#[doc(hidden)]
pub fn resolve_option_keyed_arc<T: ?Sized + Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    key: &str,
) -> Option<Arc<T>> {
    let ty = std::any::type_name::<T>();
    resolver
        .get_keyed_any(ty, key)
        .and_then(|a| a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d)))
}

/// Polymorphic `Vec<Arc<T>>` inject field.
#[doc(hidden)]
pub fn resolve_vec_arc<T: ?Sized + Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    consumer: &'static str,
    field: &'static str,
) -> Result<Vec<Arc<T>>, RdiError> {
    let ty = std::any::type_name::<T>();
    let ctx = field_context(consumer, field);
    let mut out = Vec::new();
    for a in resolver.get_all_any(ty) {
        let d = a.downcast::<Arc<T>>().map_err(|_| RdiError::FactoryPanic {
            service: ty.to_string(),
            payload: format!("polymorphic inject type mismatch for {ctx}"),
        })?;
        out.push(Arc::clone(&*d));
    }
    Ok(out)
}

/// Owned inject field (`get_owned` + try_unwrap).
#[doc(hidden)]
pub fn resolve_owned<T: Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    consumer: &'static str,
    field: &'static str,
) -> Result<T, RdiError> {
    let ty = std::any::type_name::<T>();
    let ctx = field_context(consumer, field);
    let arc_any = resolver
        .get_owned_any(ty)
        .ok_or_else(|| RdiError::not_found(ty, Some(ctx.clone())))?;
    let arc_t: Arc<T> = arc_any
        .downcast::<Arc<T>>()
        .ok()
        .map(|d| Arc::clone(&*d))
        .ok_or_else(|| RdiError::not_found(ty, Some(ctx.clone())))?;
    Arc::try_unwrap(arc_t).map_err(|_| RdiError::OwnedNotSupported(ty))
}

/// Keyed owned inject field.
#[doc(hidden)]
pub fn resolve_keyed_owned<T: Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    key: &str,
    consumer: &'static str,
    field: &'static str,
) -> Result<T, RdiError> {
    let ty = std::any::type_name::<T>();
    let ctx = field_context(consumer, field);
    let arc_any = resolver
        .get_keyed_owned_any(ty, key)
        .ok_or_else(|| RdiError::keyed_not_found(ty, key, Some(ctx.clone())))?;
    let arc_t: Arc<T> = arc_any
        .downcast::<Arc<T>>()
        .ok()
        .map(|d| Arc::clone(&*d))
        .ok_or_else(|| RdiError::keyed_not_found(ty, key, Some(ctx)))?;
    Arc::try_unwrap(arc_t).map_err(|_| RdiError::OwnedNotSupported(ty))
}

/// Optional owned inject field.
#[doc(hidden)]
pub fn resolve_option_owned<T: Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
) -> Option<T> {
    let ty = std::any::type_name::<T>();
    let arc_any = resolver.get_owned_any(ty)?;
    let arc_t: Arc<T> = arc_any.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d))?;
    Arc::try_unwrap(arc_t).ok()
}

/// Optional keyed owned inject field.
#[doc(hidden)]
pub fn resolve_option_keyed_owned<T: Send + Sync + 'static>(
    resolver: &dyn IServiceResolver,
    key: &str,
) -> Option<T> {
    let ty = std::any::type_name::<T>();
    let arc_any = resolver.get_keyed_owned_any(ty, key)?;
    let arc_t: Arc<T> = arc_any.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d))?;
    Arc::try_unwrap(arc_t).ok()
}

/// `#[inject(provider)]` — requires a real [`ServiceProvider`] backing the resolver.
///
/// The build-time validation resolver is backed by the provider being built, so
/// this works for both eagerly built singletons and lazily resolved services.
#[doc(hidden)]
pub fn resolve_provider(
    resolver: &dyn IServiceResolver,
    consumer: &'static str,
    field: &'static str,
) -> Result<Arc<ServiceProvider>, RdiError> {
    resolver.provider_arc().ok_or_else(|| RdiError::FactoryPanic {
        service: consumer.to_string(),
        payload: format!(
            "{consumer}::{field}: ServiceProvider not available; build()/build_async() returns Arc<ServiceProvider> which enables #[inject(provider)]"
        ),
    })
}

/// Type-erased Any downcast helper kept for macro edge cases.
#[doc(hidden)]
pub fn downcast_arc<T: ?Sized + Send + Sync + 'static>(
    a: Arc<dyn Any + Send + Sync>,
) -> Option<Arc<T>> {
    a.downcast::<Arc<T>>().ok().map(|d| Arc::clone(&*d))
}
