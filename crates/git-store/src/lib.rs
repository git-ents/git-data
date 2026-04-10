//! Transactional, typed, structured data over git objects and refs.

// We can remove this when the crate is fully written.
#![allow(unused)]

pub mod store;

#[cfg(test)]
mod tests;

use std::hash::Hash;

/// An interface for any content-addressable store (CAS).
///
/// `store` is pure: same input always yields the same hash. `retrieve(store(v)) == v`.
/// Two values hash identically if and only if they are equal.
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

/// A named pointer into a [`ContentAddressable`] store. Read-only.
pub trait Ref {
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
/// | `expected`   | `new`        | meaning |
/// |--------------|--------------|---------|
/// | `None`       | `Some(h)`    | create  |
/// | `Some(old)`  | `Some(new)`  | update  |
/// | `Some(old)`  | `None`       | delete  |
pub trait Transaction {
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
