//! Plumbing commands for tracking non-text objects.

use std::hash::Hash;

/// Plumbing functionality for non-text object storage.
pub struct Store {
    repo: git2::Repository,
}

impl Store {
    /// Returns a reference to the underlying repository.
    #[must_use]
    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }
}

/// An interface for any content-addressable store (CAS).
///
/// All implementations must satisfy the following invariants.
///
/// 1. The `store` method is a pure function over `v`; `store(v)` always returns the same hash for the same `v`.
/// 2. The `retrieve` and `store` methods are inverse operations over `v`; `retrieve(store(v))` is always equivalent to `v`.
/// 3. If `G == store(c)` and `G == store(v)`, then `v == c`.
pub trait ContentAddressable {
    /// The hash type used to identify stored values.
    type Hash: Eq + Clone + Hash;
    /// The value type stored in this CAS.
    type Value;
    /// The error type returned by operations on this store.
    type Error;

    /// Store a value and return its hash.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be stored.
    fn store(&self, value: &Self::Value) -> Result<Self::Hash, Self::Error>;

    /// Retrieve a value by its hash.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be read.
    fn retrieve(&self, hash: &Self::Hash) -> Result<Option<Self::Value>, Self::Error>;

    /// Check whether a value with the given hash exists in the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the store cannot be read.
    fn contains(&self, hash: &Self::Hash) -> Result<bool, Self::Error>;
}

/// A reference to a value in a `ContentAddressable` store.
pub trait Pointer {
    /// The hash type used to identify stored values.
    type Hash: Eq + Clone;
    /// The error type returned by operations on this pointer.
    type Error;

    /// Read the current hash this pointer refers to.
    ///
    /// # Errors
    ///
    /// Returns an error if the pointer cannot be read.
    fn read(&self) -> Result<Option<Self::Hash>, Self::Error>;

    /// Atomically update this pointer from `expected` to `new`.
    ///
    /// # Errors
    ///
    /// Returns an error if the compare-and-swap fails or the pointer cannot be updated.
    fn cas(&self, expected: Option<Self::Hash>, new: Self::Hash) -> Result<(), Self::Error>;
}
