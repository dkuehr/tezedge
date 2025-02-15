// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

//! Implementation of a repository that is accessed through IPC calls.
//! It is used by read-only protocol runners to be able to access the in-memory context
//! owned by the writable protocol runner.

use std::{borrow::Cow, path::Path, sync::Arc};

use crypto::hash::ContextHash;
use slog::{error, info};
use tezos_timing::RepositoryMemoryUsage;
use thiserror::Error;

use crate::persistent::{DBError, Flushable, Persistable};
use crate::working_tree::shape::{DirectoryShapeId, ShapeStrings};
use crate::working_tree::storage::DirEntryId;
use crate::working_tree::string_interner::{StringId, StringInterner};
use crate::ContextValue;
use crate::{
    ffi::TezedgeIndexError, gc::NotGarbageCollected, persistent::KeyValueStoreBackend, ObjectHash,
};

pub struct ReadonlyIpcBackend {
    client: IpcContextClient,
    hashes: HashValueStore,
}

// TODO - TE-261: quick hack to make the initializer happy, but must be fixed.
// Probably needs a separate thread for the controller, and communication
// should happen through a channel.
unsafe impl Send for ReadonlyIpcBackend {}
unsafe impl Sync for ReadonlyIpcBackend {}

impl ReadonlyIpcBackend {
    /// Connects the IPC backend to a socket in `socket_path`. This operation is blocking.
    /// Will wait for a few seconds if the socket file is not found yet.
    pub fn try_connect<P: AsRef<Path>>(socket_path: P) -> Result<Self, IpcError> {
        let client = IpcContextClient::try_connect(socket_path)?;
        Ok(Self {
            client,
            hashes: HashValueStore::new(None),
        })
    }
}

impl NotGarbageCollected for ReadonlyIpcBackend {}

impl KeyValueStoreBackend for ReadonlyIpcBackend {
    fn write_batch(&mut self, _batch: Vec<(HashId, Arc<[u8]>)>) -> Result<(), DBError> {
        // This context is readonly
        Ok(())
    }

    fn contains(&self, hash_id: HashId) -> Result<bool, DBError> {
        if let Some(hash_id) = hash_id.get_readonly_id()? {
            self.hashes.contains(hash_id).map_err(Into::into)
        } else {
            self.client
                .contains_object(hash_id)
                .map_err(|reason| DBError::IpcAccessError { reason })
        }
    }

    fn put_context_hash(&mut self, _hash_id: HashId) -> Result<(), DBError> {
        // This context is readonly
        Ok(())
    }

    fn get_context_hash(&self, context_hash: &ContextHash) -> Result<Option<HashId>, DBError> {
        self.client
            .get_context_hash_id(context_hash)
            .map_err(|reason| DBError::IpcAccessError { reason })
    }

    fn get_hash(&self, hash_id: HashId) -> Result<Option<Cow<ObjectHash>>, DBError> {
        if let Some(hash_id) = hash_id.get_readonly_id()? {
            Ok(self.hashes.get_hash(hash_id)?.map(Cow::Borrowed))
        } else {
            self.client
                .get_hash(hash_id)
                .map_err(|reason| DBError::IpcAccessError { reason })
        }
    }

    fn get_value(&self, hash_id: HashId) -> Result<Option<Cow<[u8]>>, DBError> {
        if let Some(hash_id) = hash_id.get_readonly_id()? {
            Ok(self.hashes.get_value(hash_id)?.map(Cow::Borrowed))
        } else {
            self.client
                .get_value(hash_id)
                .map_err(|reason| DBError::IpcAccessError { reason })
        }
    }

    fn get_vacant_object_hash(&mut self) -> Result<VacantObjectHash, DBError> {
        self.hashes
            .get_vacant_object_hash()?
            .set_readonly_runner()
            .map_err(Into::into)
    }

    fn clear_objects(&mut self) -> Result<(), DBError> {
        self.hashes.clear();
        Ok(())
    }

    fn memory_usage(&self) -> RepositoryMemoryUsage {
        self.hashes.get_memory_usage()
    }

    fn get_shape(&self, shape_id: DirectoryShapeId) -> Result<ShapeStrings, DBError> {
        self.client
            .get_shape(shape_id)
            .map(ShapeStrings::Owned)
            .map_err(|reason| DBError::IpcAccessError { reason })
    }

    fn make_shape(
        &mut self,
        _dir: &[(StringId, DirEntryId)],
    ) -> Result<Option<DirectoryShapeId>, DBError> {
        // Readonly protocol runner doesn't make shapes.
        Ok(None)
    }

    fn get_str(&self, _: StringId) -> Option<&str> {
        // Readonly protocol runner doesn't have the `StringInterner`.
        None
    }

    fn synchronize_strings(&mut self, _string_interner: &StringInterner) -> Result<(), DBError> {
        // Readonly protocol runner doesn't update strings.
        Ok(())
    }
}

impl Flushable for ReadonlyIpcBackend {
    fn flush(&self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

impl Persistable for ReadonlyIpcBackend {
    fn is_persistent(&self) -> bool {
        false
    }
}

// IPC communication

use std::{cell::RefCell, time::Duration};

use ipc::{IpcClient, IpcError, IpcReceiver, IpcSender, IpcServer};
use serde::{Deserialize, Serialize};
use slog::{warn, Logger};
use strum_macros::IntoStaticStr;

use super::{in_memory::HashValueStore, HashId, VacantObjectHash};

/// This request is generated by a readonly protool runner and is received by the writable protocol runner.
#[derive(Serialize, Deserialize, Debug, IntoStaticStr)]
enum ContextRequest {
    GetContextHashId(ContextHash),
    GetHash(HashId),
    GetValue(HashId),
    GetShape(DirectoryShapeId),
    ContainsObject(HashId),
    ShutdownCall, // TODO: is this required?
}

/// This is generated as a response to the `ContextRequest` command.
#[derive(Serialize, Deserialize, Debug, IntoStaticStr)]
enum ContextResponse {
    GetContextHashResponse(Result<Option<ObjectHash>, String>),
    GetContextHashIdResponse(Result<Option<HashId>, String>),
    GetValueResponse(Result<Option<ContextValue>, String>),
    GetShapeResponse(Result<Vec<String>, String>),
    ContainsObjectResponse(Result<bool, String>),
    ShutdownResult,
}

#[derive(Error, Debug)]
pub enum ContextError {
    #[error("Context get object error: {reason}")]
    GetValueError { reason: String },
    #[error("Context get shape error: {reason}")]
    GetShapeError { reason: String },
    #[error("Context contains object error: {reason}")]
    ContainsObjectError { reason: String },
    #[error("Context get hash id error: {reason}")]
    GetContextHashIdError { reason: String },
    #[error("Context get hash error: {reason}")]
    GetContextHashError { reason: String },
}

#[derive(Error, Debug)]
pub enum IpcContextError {
    #[error("Could not obtain a read lock to the TezEdge index")]
    TezedgeIndexReadLockError,
    #[error("IPC error: {reason}")]
    IpcError { reason: IpcError },
}

impl From<TezedgeIndexError> for IpcContextError {
    fn from(_: TezedgeIndexError) -> Self {
        Self::TezedgeIndexReadLockError
    }
}

impl From<IpcError> for IpcContextError {
    fn from(error: IpcError) -> Self {
        IpcContextError::IpcError { reason: error }
    }
}

/// Errors generated by `protocol_runner`.
#[derive(Error, Debug)]
pub enum ContextServiceError {
    /// Generic IPC communication error. See `reason` for more details.
    #[error("IPC error: {reason}")]
    IpcError { reason: IpcError },
    /// Tezos protocol error.
    #[error("Protocol error: {reason}")]
    ContextError { reason: ContextError },
    /// Unexpected message was received from IPC channel
    #[error("Received unexpected message: {message}")]
    UnexpectedMessage { message: &'static str },
    /// Lock error
    #[error("Lock error: {message:?}")]
    LockPoisonError { message: String },
}

impl<T> From<std::sync::PoisonError<T>> for ContextServiceError {
    fn from(source: std::sync::PoisonError<T>) -> Self {
        Self::LockPoisonError {
            message: source.to_string(),
        }
    }
}

impl slog::Value for ContextServiceError {
    fn serialize(
        &self,
        _record: &slog::Record,
        key: slog::Key,
        serializer: &mut dyn slog::Serializer,
    ) -> slog::Result {
        serializer.emit_arguments(key, &format_args!("{}", self))
    }
}

impl From<IpcError> for ContextServiceError {
    fn from(error: IpcError) -> Self {
        ContextServiceError::IpcError { reason: error }
    }
}

impl From<ContextError> for ContextServiceError {
    fn from(error: ContextError) -> Self {
        ContextServiceError::ContextError { reason: error }
    }
}

/// IPC context server that listens for new connections.
pub struct IpcContextListener(IpcServer<ContextRequest, ContextResponse>);

pub struct ContextIncoming<'a> {
    listener: &'a mut IpcContextListener,
}

struct IpcClientIO {
    rx: IpcReceiver<ContextResponse>,
    tx: IpcSender<ContextRequest>,
}

struct IpcServerIO {
    rx: IpcReceiver<ContextRequest>,
    tx: IpcSender<ContextResponse>,
}

/// Encapsulate IPC communication.
pub struct IpcContextClient {
    io: RefCell<IpcClientIO>,
}

pub struct IpcContextServer {
    io: RefCell<IpcServerIO>,
}

/// IPC context client for readers.
impl IpcContextClient {
    const TIMEOUT: Duration = Duration::from_secs(180);

    pub fn try_connect<P: AsRef<Path>>(socket_path: P) -> Result<Self, IpcError> {
        // TODO - TE-261: do this in a better way
        for _ in 0..5 {
            if socket_path.as_ref().exists() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        let ipc_client: IpcClient<ContextResponse, ContextRequest> = IpcClient::new(socket_path);
        let (rx, tx) = ipc_client.connect()?;
        let io = RefCell::new(IpcClientIO { rx, tx });
        Ok(Self { io })
    }

    /// Get object by hash id
    pub fn get_value(&self, hash_id: HashId) -> Result<Option<Cow<[u8]>>, ContextServiceError> {
        let mut io = self.io.borrow_mut();
        io.tx.send(&ContextRequest::GetValue(hash_id))?;

        // this might take a while, so we will use unusually long timeout
        match io
            .rx
            .try_receive(Some(Self::TIMEOUT), Some(IpcContextListener::IO_TIMEOUT))?
        {
            ContextResponse::GetValueResponse(result) => result
                .map(|h| h.map(Cow::Owned))
                .map_err(|err| ContextError::GetValueError { reason: err }.into()),
            message => Err(ContextServiceError::UnexpectedMessage {
                message: message.into(),
            }),
        }
    }

    /// Check if object with hash id exists
    pub fn contains_object(&self, hash_id: HashId) -> Result<bool, ContextServiceError> {
        let mut io = self.io.borrow_mut();
        io.tx.send(&ContextRequest::ContainsObject(hash_id))?;

        // this might take a while, so we will use unusually long timeout
        match io
            .rx
            .try_receive(Some(Self::TIMEOUT), Some(IpcContextListener::IO_TIMEOUT))?
        {
            ContextResponse::ContainsObjectResponse(result) => {
                result.map_err(|err| ContextError::ContainsObjectError { reason: err }.into())
            }
            message => Err(ContextServiceError::UnexpectedMessage {
                message: message.into(),
            }),
        }
    }

    /// Check if object with hash id exists
    pub fn get_context_hash_id(
        &self,
        context_hash: &ContextHash,
    ) -> Result<Option<HashId>, ContextServiceError> {
        let mut io = self.io.borrow_mut();
        io.tx
            .send(&ContextRequest::GetContextHashId(context_hash.clone()))?;

        // this might take a while, so we will use unusually long timeout
        match io
            .rx
            .try_receive(Some(Self::TIMEOUT), Some(IpcContextListener::IO_TIMEOUT))?
        {
            ContextResponse::GetContextHashIdResponse(result) => {
                result.map_err(|err| ContextError::GetContextHashIdError { reason: err }.into())
            }
            message => Err(ContextServiceError::UnexpectedMessage {
                message: message.into(),
            }),
        }
    }

    /// Check if object with hash id exists
    pub fn get_hash(
        &self,
        hash_id: HashId,
    ) -> Result<Option<Cow<ObjectHash>>, ContextServiceError> {
        let mut io = self.io.borrow_mut();
        io.tx.send(&ContextRequest::GetHash(hash_id))?;

        // this might take a while, so we will use unusually long timeout
        match io
            .rx
            .try_receive(Some(Self::TIMEOUT), Some(IpcContextListener::IO_TIMEOUT))?
        {
            ContextResponse::GetContextHashResponse(result) => result
                .map(|h| h.map(Cow::Owned))
                .map_err(|err| ContextError::GetContextHashError { reason: err }.into()),
            message => Err(ContextServiceError::UnexpectedMessage {
                message: message.into(),
            }),
        }
    }

    /// Get object by hash id
    pub fn get_shape(
        &self,
        shape_id: DirectoryShapeId,
    ) -> Result<Vec<String>, ContextServiceError> {
        let mut io = self.io.borrow_mut();
        io.tx.send(&ContextRequest::GetShape(shape_id))?;

        // this might take a while, so we will use unusually long timeout
        match io
            .rx
            .try_receive(Some(Self::TIMEOUT), Some(IpcContextListener::IO_TIMEOUT))?
        {
            ContextResponse::GetShapeResponse(result) => {
                result.map_err(|err| ContextError::GetShapeError { reason: err }.into())
            }
            message => Err(ContextServiceError::UnexpectedMessage {
                message: message.into(),
            }),
        }
    }
}

impl<'a> Iterator for ContextIncoming<'a> {
    type Item = Result<IpcContextServer, IpcError>;
    fn next(&mut self) -> Option<Result<IpcContextServer, IpcError>> {
        Some(self.listener.accept())
    }
}

impl IpcContextListener {
    const IO_TIMEOUT: Duration = Duration::from_secs(180);

    /// Create new IPC endpoint
    pub fn try_new<P: AsRef<Path>>(socket_path: P) -> Result<Self, IpcError> {
        // Remove file first, otherwise bind will fail.
        std::fs::remove_file(&socket_path).ok();

        Ok(IpcContextListener(IpcServer::bind_path(socket_path)?))
    }

    /// Start accepting incoming IPC connections.
    ///
    /// Returns an [`ipc context server`](IpcContextServer) if new IPC channel is successfully created.
    /// This is a blocking operation.
    pub fn accept(&mut self) -> Result<IpcContextServer, IpcError> {
        let (rx, tx) = self.0.accept()?;

        Ok(IpcContextServer {
            io: RefCell::new(IpcServerIO { rx, tx }),
        })
    }

    /// Returns an iterator over the connections being received on this context IPC listener.
    pub fn incoming(&mut self) -> ContextIncoming<'_> {
        ContextIncoming { listener: self }
    }

    /// Starts accepting connections.
    ///
    /// A new thread is launched to serve each connection.
    pub fn handle_incoming_connections(&mut self, log: &Logger) {
        for connection in self.incoming() {
            match connection {
                Err(err) => {
                    error!(&log, "Error accepting IPC connection"; "reason" => format!("{:?}", err))
                }
                Ok(server) => {
                    info!(
                        &log,
                        "IpcContextServer accepted new IPC connection for context"
                    );
                    let log_inner = log.clone();
                    if let Err(spawn_error) = std::thread::Builder::new()
                        .name("ctx-ipc-server-thread".to_string())
                        .spawn(move || {
                            if let Err(err) = server.process_context_requests(&log_inner) {
                                error!(
                                    &log_inner,
                                    "Error when processing context IPC requests";
                                    "reason" => format!("{:?}", err),
                                );
                            }
                        })
                    {
                        error!(
                            &log,
                            "Failed to spawn thread to IpcContextServer";
                            "reason" => spawn_error,
                        );
                    }
                }
            }
        }
    }
}

impl IpcContextServer {
    /// Listen to new connections from context readers.
    /// Begin receiving commands from context readers until `ShutdownCall` command is received.
    pub fn process_context_requests(&self, log: &Logger) -> Result<(), IpcContextError> {
        let mut io = self.io.borrow_mut();
        loop {
            let cmd = io.rx.receive()?;

            match cmd {
                ContextRequest::GetValue(hash) => match crate::ffi::get_context_index()? {
                    None => io.tx.send(&ContextResponse::GetValueResponse(Err(
                        "Context index unavailable".to_owned(),
                    )))?,
                    Some(index) => {
                        let res = index
                            .fetch_object_bytes(hash)
                            .map_err(|err| format!("Context error: {:?}", err));
                        io.tx.send(&ContextResponse::GetValueResponse(res))?;
                    }
                },
                ContextRequest::GetShape(shape_id) => match crate::ffi::get_context_index()? {
                    None => io.tx.send(&ContextResponse::GetShapeResponse(Err(
                        "Context index unavailable".to_owned(),
                    )))?,
                    Some(index) => {
                        let res = index
                            .repository
                            .read()
                            .map_err(|_| ContextError::GetShapeError {
                                reason: "Fail to get repo".to_string(),
                            })
                            .and_then(|repo| {
                                let shape = repo.get_shape(shape_id).map_err(|_| {
                                    ContextError::GetShapeError {
                                        reason: "Fail to get shape".to_string(),
                                    }
                                })?;

                                // We send the owned `String` to the read only protocol runner.
                                // We do not send the `StringId`s because the read only protocol
                                // runner doesn't have access to the same `StringInterner`.
                                match shape {
                                    ShapeStrings::SliceIds(slice_ids) => slice_ids
                                        .iter()
                                        .map(|s| {
                                            repo.get_str(*s)
                                                .ok_or_else(|| ContextError::GetShapeError {
                                                    reason: format!("String not found"),
                                                })
                                                .map(|s| s.to_string())
                                        })
                                        .collect(),
                                    ShapeStrings::Owned(_) => Err(ContextError::GetShapeError {
                                        reason: "Should receive a slice of StringId".to_string(),
                                    }),
                                }
                            })
                            .map_err(|err| format!("Context error: {:?}", err));

                        io.tx.send(&ContextResponse::GetShapeResponse(res))?;
                    }
                },
                ContextRequest::ContainsObject(hash) => match crate::ffi::get_context_index()? {
                    None => io.tx.send(&ContextResponse::GetValueResponse(Err(
                        "Context index unavailable".to_owned(),
                    )))?,
                    Some(index) => {
                        let res = index
                            .contains(hash)
                            .map_err(|err| format!("Context error: {:?}", err));
                        io.tx.send(&ContextResponse::ContainsObjectResponse(res))?;
                    }
                },

                ContextRequest::ShutdownCall => {
                    if let Err(e) = io.tx.send(&ContextResponse::ShutdownResult) {
                        warn!(log, "Failed to send shutdown response"; "reason" => format!("{}", e));
                    }

                    break;
                }
                ContextRequest::GetContextHashId(context_hash) => {
                    match crate::ffi::get_context_index()? {
                        None => io.tx.send(&ContextResponse::GetContextHashIdResponse(Err(
                            "Context index unavailable".to_owned(),
                        )))?,
                        Some(index) => {
                            let res = index
                                .fetch_context_hash_id(&context_hash)
                                .map_err(|err| format!("Context error: {:?}", err));

                            io.tx
                                .send(&ContextResponse::GetContextHashIdResponse(res))?;
                        }
                    }
                }
                ContextRequest::GetHash(hash_id) => match crate::ffi::get_context_index()? {
                    None => io.tx.send(&ContextResponse::GetContextHashResponse(Err(
                        "Context index unavailable".to_owned(),
                    )))?,
                    Some(index) => {
                        let res = index
                            .fetch_hash(hash_id)
                            .map_err(|err| format!("Context error: {:?}", err));

                        io.tx.send(&ContextResponse::GetContextHashResponse(res))?;
                    }
                },
            }
        }

        Ok(())
    }
}
