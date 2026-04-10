//! Plumbing commands for tracking non-text objects.

// We can remove this when the crate is fully written.
#![allow(unused)]

#[cfg(test)]
mod tests;

use std::{error::Error, hash::Hash};

/// An interface for any content-addressable store (CAS).
///
/// All implementations must satisfy the following invariants.
///
/// 1. The `store` method is a pure function over `v`; `store(v)` always returns the same hash for the same `v`.
/// 2. The `retrieve` and `store` methods are inverse operations over `v`; `retrieve(store(v))` is always equivalent to `v`.
/// 3. For `G`, `c`, and `v`: `G == store(c)` and `G == store(v)` if and only if `c == v`.
pub(crate) trait ContentAddressable {
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

/// A named pointer into a [`ContentAddressable`] store. Read-only.
pub(crate) trait Ref {
    /// The hash type used to identify stored values.
    type Hash: Eq + Clone;
    /// The error type returned by operations on this ref.
    type Error;

    /// Read the current hash this ref points to.
    ///
    /// # Errors
    ///
    /// Returns an error if the ref cannot be read.
    fn read(&self) -> Result<Option<Self::Hash>, Self::Error>;
}

/// A batch of guarded ref updates, committed atomically.
///
/// Stage one or more `(ref, expected, new)` updates, then call [`commit`](Transaction::commit)
/// to apply them all-or-nothing. A single-ref compare-and-swap is a one-entry transaction.
///
/// | `expected` | `new`      | meaning |
/// |------------|------------|---------|
/// | `None`     | `Some(h)`  | create  |
/// | `Some(old)`| `Some(new)`| update  |
/// | `Some(old)`| `None`     | delete  |
pub(crate) trait Transaction {
    /// The ref type this transaction operates on.
    type Ref: Ref;
    /// The error type returned by commit.
    type Error;

    /// Stage a guarded update for the given ref.
    fn stage(
        &mut self,
        pointer: &Self::Ref,
        expected: Option<<Self::Ref as Ref>::Hash>,
        new: Option<<Self::Ref as Ref>::Hash>,
    );

    /// Atomically commit all staged updates.
    ///
    /// # Errors
    ///
    /// Returns an error if any guard fails or the updates cannot be applied.
    fn commit(self) -> Result<(), Self::Error>;
}

/// A branch of entry objects with support for multiple parents.
pub(crate) trait Chain {
    type Store: ContentAddressable;
    type Entry;
    type Error;

    /// Returns the hash of the most recent entry in the chain.
    fn head(&self) -> Result<Option<<Self::Store as ContentAddressable>::Hash>, Self::Error>;

    /// Appends a new entry to the chain, returning the hash of the new entry.
    fn append(
        &mut self,
        entry: Self::Entry,
    ) -> Result<<Self::Store as ContentAddressable>::Hash, Self::Error>;

    /// Returns an iterator over the entries in the chain, starting from the head.
    fn log(&self) -> Result<impl Iterator<Item = Self::Entry>, Self::Error>;
}

/// A key-value store with its own semantics.
pub(crate) trait Ledger {
    type Store: ContentAddressable;
    type Key;
    type Value;
    type Error;

    /// Returns the value associated with the given key, if one exists.
    fn get(&self, key: &Self::Key) -> Result<Option<Self::Value>, Self::Error>;

    /// Associates the given value with the given key, returning the hash of the new entry.
    fn put(
        &mut self,
        key: Self::Key,
        value: Self::Value,
    ) -> Result<<Self::Store as ContentAddressable>::Hash, Self::Error>;

    /// Deletes the value associated with the given key, returning the hash of the deleted entry.
    fn delete(
        &mut self,
        key: &Self::Key,
    ) -> Result<<Self::Store as ContentAddressable>::Hash, Self::Error>;

    /// Returns an iterator over the key-value pairs in the ledger.
    fn list(&self) -> Result<impl Iterator<Item = (Self::Key, Self::Value)>, Self::Error>;
}
