//! [`GitStore`]: a [`ContentAddressable`] store and ref operations backed by a gix repository.

use gix::ObjectId;
use gix::refs::Target;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

use crate::{ContentAddressable, Ref, Transaction};

/// Errors returned by [`GitStore`] and related types.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Failed to open a repository.
    #[error("failed to open repository: {0}")]
    Open(#[from] Box<gix::open::Error>),

    /// Failed to initialize a repository.
    #[error("failed to initialize repository: {0}")]
    Init(#[from] Box<gix::init::Error>),

    /// Failed to write an object.
    #[error("failed to write object: {0}")]
    WriteObject(#[from] gix::object::write::Error),

    /// Failed to read an object.
    #[error("failed to read object: {0}")]
    FindObject(gix::object::find::existing::Error),

    /// Failed to find a reference.
    #[error("failed to find reference: {0}")]
    FindRef(#[from] gix::reference::find::Error),

    /// Failed to edit references.
    #[error("failed to edit references: {0}")]
    EditRef(#[from] gix::reference::edit::Error),

    /// Reference name is not valid.
    #[error("invalid reference name: {0}")]
    InvalidRefName(String),
}

/// A git repository acting as a content-addressable blob store.
pub struct GitStore {
    pub(crate) repo: gix::Repository,
}

impl GitStore {
    /// Open an existing repository.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Open`] if the path is not a git repository.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        Ok(Self {
            repo: gix::open(path.as_ref().to_path_buf()).map_err(Box::new)?,
        })
    }

    /// Initialize a new repository.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Init`] if the repository cannot be created.
    pub fn init(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        Ok(Self {
            repo: gix::init(path).map_err(Box::new)?,
        })
    }

    /// Construct a [`GitRef`] for the given fully-qualified ref name.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRefName`] if `name` is not a valid git ref name.
    pub fn git_ref(&self, name: &str) -> Result<GitRef<'_>, Error> {
        let name = name
            .try_into()
            .map_err(|_| Error::InvalidRefName(name.to_owned()))?;
        Ok(GitRef { store: self, name })
    }

    /// Begin a new ref transaction against this store.
    pub fn transaction(&self) -> GitTx<'_> {
        GitTx {
            store: self,
            staged: Vec::new(),
        }
    }
}

impl ContentAddressable for GitStore {
    type Hash = ObjectId;
    type Value = Vec<u8>;
    type Error = Error;

    fn store(&self, value: &Vec<u8>) -> Result<ObjectId, Error> {
        Ok(self.repo.write_blob(value.as_slice())?.detach())
    }

    fn retrieve(&self, hash: &ObjectId) -> Result<Option<Vec<u8>>, Error> {
        match self.repo.find_object(*hash) {
            Ok(obj) => Ok(Some(obj.data.clone())),
            Err(gix::object::find::existing::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(Error::FindObject(e)),
        }
    }

    fn contains(&self, hash: &ObjectId) -> Result<bool, Error> {
        match self.repo.find_object(*hash) {
            Ok(_) => Ok(true),
            Err(gix::object::find::existing::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(Error::FindObject(e)),
        }
    }
}

/// A named git ref backed by a [`GitStore`].
pub struct GitRef<'a> {
    store: &'a GitStore,
    name: gix::refs::FullName,
}

impl Ref for GitRef<'_> {
    type Hash = ObjectId;
    type Error = Error;

    fn read(&self) -> Result<Option<ObjectId>, Error> {
        match self.store.repo.try_find_reference(self.name.as_ref())? {
            Some(r) => Ok(Some(r.id().detach())),
            None => Ok(None),
        }
    }
}

/// An in-progress ref transaction against a [`GitStore`].
pub struct GitTx<'a> {
    store: &'a GitStore,
    staged: Vec<RefEdit>,
}

impl<'a> Transaction for GitTx<'a> {
    type Ref = GitRef<'a>;
    type Error = Error;

    fn stage(&mut self, pointer: &GitRef<'a>, expected: Option<ObjectId>, new: Option<ObjectId>) {
        let expected_prev = match expected {
            Some(oid) => PreviousValue::MustExistAndMatch(Target::Object(oid)),
            None => PreviousValue::MustNotExist,
        };
        let change = match new {
            Some(oid) => Change::Update {
                log: LogChange::default(),
                expected: expected_prev,
                new: Target::Object(oid),
            },
            None => Change::Delete {
                expected: expected_prev,
                log: RefLog::AndReference,
            },
        };
        self.staged.push(RefEdit {
            change,
            name: pointer.name.clone(),
            deref: false,
        });
    }

    fn commit(self) -> Result<(), Error> {
        self.store.repo.edit_references(self.staged)?;
        Ok(())
    }
}
