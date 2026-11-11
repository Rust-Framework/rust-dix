//! Service keys for keyed registrations.
//!
//! A key is either a **typed value** (`Eq + Hash + Debug`, any type) or the
//! special [`ServiceKey::any_key`] sentinel used for "fallback for any key"
//! registrations (MEDI `KeyedService.AnyKey` parity).
//!
//! String keys used by `keyed_singleton("name", …)` are just typed keys whose
//! value is a `String`, so existing string-keyed code keeps working while
//! typed keys enable MEDI-style keyed services (enums, integers, …).

use std::any::Any;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// Erased key value. Implemented for every `T: Any + Send + Sync + Eq + Hash + Debug`.
#[doc(hidden)]
pub trait ErasedKeyValue: Any + Send + Sync + fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    fn key_eq(&self, other: &dyn Any) -> bool;
    fn key_hash(&self) -> u64;
}

impl<T> ErasedKeyValue for T
where
    T: Any + Send + Sync + Eq + Hash + fmt::Debug,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }

    fn key_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone)]
enum KeyRepr {
    Value(Arc<dyn ErasedKeyValue>),
    Any,
}

/// Key of a keyed service registration / resolution.
///
/// Construct a value key with [`ServiceKey::new`] (or `From<&str>`/`From<String>`
/// for string keys), and the fallback sentinel with [`ServiceKey::any_key`].
///
/// # String keys
///
/// String-keyed registration APIs (`keyed_singleton("name", …)`) store the key
/// as a `String`. Use `ServiceKey::from("name")` / `"name".into()` to build an
/// equal key. `ServiceKey::new("name")` is a **different** key type (`&str`),
/// because key equality includes the value type — use it only for intentional
/// `&'static str`-typed keys.
///
/// # Reading a requested key
///
/// `AnyKey` fallback factories receive `&ServiceKey`. The erased key exposes
/// `Debug`/`Display` and equality/hash; there is intentionally **no** typed
/// downcast accessor (operations must be expressible through the `ServiceKey`
/// API itself, keeping the erased-key representation private).
#[derive(Clone)]
pub struct ServiceKey(KeyRepr);

impl ServiceKey {
    /// Create a typed key value.
    ///
    /// ```
    /// use rust_dix::ServiceKey;
    ///
    /// #[derive(Debug, PartialEq, Eq, Hash)]
    /// enum Db { Read, Write }
    ///
    /// let key = ServiceKey::new(Db::Read);
    /// assert_ne!(key, ServiceKey::new(Db::Write));
    /// ```
    pub fn new<T>(value: T) -> Self
    where
        T: Any + Send + Sync + Eq + Hash + fmt::Debug + 'static,
    {
        Self(KeyRepr::Value(Arc::new(value)))
    }

    /// The `AnyKey` fallback sentinel (MEDI `KeyedService.AnyKey`).
    ///
    /// Registering a service under this key makes it a fallback for **every**
    /// key that has no exact registration. Resolving a single service *with*
    /// this sentinel is not allowed (use [`ServiceKey`]-keyed lookups or
    /// enumeration instead).
    pub fn any_key() -> Self {
        Self(KeyRepr::Any)
    }

    /// Whether this key is the [`ServiceKey::any_key`] sentinel.
    pub fn is_any(&self) -> bool {
        matches!(self.0, KeyRepr::Any)
    }

    /// Stable hash used for per-requested-key cycle markers.
    pub(crate) fn stable_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl From<&str> for ServiceKey {
    fn from(value: &str) -> Self {
        Self::new(value.to_owned())
    }
}

impl From<String> for ServiceKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&String> for ServiceKey {
    fn from(value: &String) -> Self {
        Self::new(value.clone())
    }
}

impl From<&ServiceKey> for ServiceKey {
    fn from(value: &ServiceKey) -> Self {
        value.clone()
    }
}

impl fmt::Debug for ServiceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            KeyRepr::Value(v) => write!(f, "{v:?}"),
            KeyRepr::Any => f.write_str("AnyKey"),
        }
    }
}

impl fmt::Display for ServiceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            KeyRepr::Value(v) => {
                // String keys display without quotes (nicer error messages);
                // other typed keys fall back to their `Debug` representation.
                if let Some(s) = v.as_any().downcast_ref::<String>() {
                    f.write_str(s)
                } else {
                    write!(f, "{v:?}")
                }
            }
            KeyRepr::Any => f.write_str("AnyKey"),
        }
    }
}

impl PartialEq for ServiceKey {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (KeyRepr::Any, KeyRepr::Any) => true,
            (KeyRepr::Value(a), KeyRepr::Value(b)) => {
                a.as_any().type_id() == b.as_any().type_id() && a.key_eq(b.as_any())
            }
            _ => false,
        }
    }
}

impl Eq for ServiceKey {}

impl Hash for ServiceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.0 {
            KeyRepr::Any => state.write_u8(0),
            KeyRepr::Value(v) => {
                state.write_u8(1);
                v.as_any().type_id().hash(state);
                v.key_hash().hash(state);
            }
        }
    }
}

/// MEDI-inspired namespace for the `AnyKey` sentinel.
///
/// `KeyedService::any_key()` equals `ServiceKey::any_key()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyedService;

impl KeyedService {
    /// The `AnyKey` fallback sentinel (MEDI `KeyedService.AnyKey`).
    pub fn any_key() -> ServiceKey {
        ServiceKey::any_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Debug, PartialEq, Eq, Hash)]
    enum Db {
        Read,
        Write,
    }

    #[test]
    fn typed_keys_compare_and_hash() {
        let mut set = HashSet::new();
        set.insert(ServiceKey::new(Db::Read));
        set.insert(ServiceKey::new(Db::Write));
        set.insert(ServiceKey::new(Db::Read));
        assert_eq!(set.len(), 2);
        assert_eq!(ServiceKey::new(Db::Read), ServiceKey::new(Db::Read));
        assert_ne!(ServiceKey::new(Db::Read), ServiceKey::new(Db::Write));
    }

    #[test]
    fn string_keys_are_typed_keys() {
        assert_eq!(ServiceKey::from("a"), ServiceKey::from("a".to_string()));
        assert_ne!(ServiceKey::from("a"), ServiceKey::from("b"));
        // Different key types with the same debug/display must not collide.
        assert_ne!(ServiceKey::new(1_u32), ServiceKey::new(1_u64));
    }

    #[test]
    fn any_key_identity() {
        assert!(ServiceKey::any_key().is_any());
        assert_eq!(ServiceKey::any_key(), KeyedService::any_key());
        assert_ne!(ServiceKey::any_key(), ServiceKey::from("AnyKey"));
        assert_ne!(ServiceKey::any_key(), ServiceKey::new("AnyKey"));
    }

    #[test]
    fn debug_and_display_are_stable_enough_for_errors() {
        assert_eq!(format!("{:?}", ServiceKey::any_key()), "AnyKey");
        assert_eq!(format!("{}", ServiceKey::from("x")), "x");
        assert_eq!(format!("{:?}", ServiceKey::from("x")), "\"x\"");
    }

    #[test]
    fn string_literal_typed_key_is_a_distinct_type() {
        // `ServiceKey::new("x")` is an `&'static str` key, not the `String` key
        // produced by string-keyed registration APIs.
        assert_ne!(ServiceKey::new("x"), ServiceKey::from("x"));
    }
}
