// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::{future::Future, pin::Pin, task::Poll};

use bytes::{Buf, BufMut};
use failure::Fail;
use failure::_core::convert::TryFrom;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crypto::blake2b::{self, Blake2bError};
use crypto::hash::Hash;
use pin_project_lite::pin_project;
use tezos_encoding::binary_writer;
use tezos_encoding::encoding::HasEncoding;
use tezos_encoding::json_writer::JsonWriter;
use tezos_encoding::ser;
use tezos_encoding::{
    binary_async_reader::{BinaryAsyncReader, BinaryRead},
    de::from_value as deserialize_from_value,
};
use tezos_encoding::{
    binary_reader::{BinaryReader, BinaryReaderError},
    binary_writer::BinaryWriterError,
};
use tokio::io::AsyncRead;

use crate::p2p::binary_message::MessageHashError::SerializationError;

/// Size in bytes of the content length field
pub const CONTENT_LENGTH_FIELD_BYTES: usize = 2;
/// Max allowed message length in bytes
pub const CONTENT_LENGTH_MAX: usize = u16::max_value() as usize;

/// This feature can provide cache mechanism for BinaryMessages.
/// Cache is used to reduce computation time of encoding/decoding process.
///
/// When we use cache (see macro [cached_data]):
/// - first time we read [from_bytes], original bytes are stored to cache and message/struct is constructed from bytes
/// - so next time we want to use/call [as_bytes], bytes are not calculated with encoding, but just returned from cache
///
/// e.g: this is used, when we receive data from p2p as bytes, and then store them also as bytes to storage and calculate count of bytes in monitoring
///
/// When we dont need cache (see macro [non_cached_data]):
///
/// e.g.: we we just want to read data from bytes and never convert back to bytes
///
pub mod cache {
    use std::fmt;

    use serde::{Deserialize, Deserializer};

    pub trait CacheReader {
        fn get(&self) -> Option<Vec<u8>>;
    }

    pub trait CacheWriter {
        fn put(&mut self, body: &[u8]);
    }

    pub trait CachedData {
        fn has_cache() -> bool;
        fn cache_reader(&self) -> Option<&dyn CacheReader>;
        fn cache_writer(&mut self) -> Option<&mut dyn CacheWriter>;
    }

    #[derive(Clone, Default)]
    pub struct BinaryDataCache {
        pub(crate) data: Option<Vec<u8>>,
    }

    impl CacheReader for BinaryDataCache {
        #[inline]
        fn get(&self) -> Option<Vec<u8>> {
            self.data.as_ref().cloned()
        }
    }

    impl CacheWriter for BinaryDataCache {
        #[inline]
        fn put(&mut self, body: &[u8]) {
            self.data.replace(body.to_vec());
        }
    }

    impl PartialEq for BinaryDataCache {
        #[inline]
        fn eq(&self, _: &Self) -> bool {
            true
        }
    }

    impl Eq for BinaryDataCache {}

    impl<'de> Deserialize<'de> for BinaryDataCache {
        fn deserialize<D>(_: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            Ok(BinaryDataCache::default())
        }
    }

    impl fmt::Debug for BinaryDataCache {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.get() {
                Some(data) => write!(
                    f,
                    "BinaryDataCache {{ has_value: true, len: {} }}",
                    data.len()
                ),
                None => write!(f, "BinaryDataCache {{ has_value: false }}"),
            }
        }
    }

    #[derive(Clone, PartialEq, Default)]
    pub struct NeverCache;

    impl<'de> Deserialize<'de> for NeverCache {
        fn deserialize<D>(_: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            Ok(NeverCache::default())
        }
    }

    impl CacheReader for NeverCache {
        #[inline]
        fn get(&self) -> Option<Vec<u8>> {
            None
        }
    }

    impl CacheWriter for NeverCache {
        #[inline]
        fn put(&mut self, _: &[u8]) {
            // ..
        }
    }

    impl fmt::Debug for NeverCache {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "NeverCache {{ }}")
        }
    }

    /// Adds implementation CachedData for given struct
    /// Struct should contains property [$property_cache_name] with cache struct, e.g. BinaryDataCache,
    /// usually this cache does not need to be serialized, so can be marked with [#[serde(skip_serializing)]]
    #[macro_export]
    macro_rules! cached_data {
        ($struct_name:ident, $property_cache_name:ident) => {
            impl $crate::p2p::binary_message::cache::CachedData for $struct_name {
                #[inline]
                fn has_cache() -> bool {
                    true
                }

                #[inline]
                fn cache_reader(
                    &self,
                ) -> Option<&dyn $crate::p2p::binary_message::cache::CacheReader> {
                    Some(&self.$property_cache_name)
                }

                #[inline]
                fn cache_writer(
                    &mut self,
                ) -> Option<&mut dyn $crate::p2p::binary_message::cache::CacheWriter> {
                    Some(&mut self.$property_cache_name)
                }
            }
        };
    }

    /// Adds empty non-caching implementation CachedData for given struct
    #[macro_export]
    macro_rules! non_cached_data {
        ($struct_name:ident) => {
            impl $crate::p2p::binary_message::cache::CachedData for $struct_name {
                #[inline]
                fn has_cache() -> bool {
                    true
                }

                #[inline]
                fn cache_reader(
                    &self,
                ) -> Option<&dyn $crate::p2p::binary_message::cache::CacheReader> {
                    None
                }

                #[inline]
                fn cache_writer(
                    &mut self,
                ) -> Option<&mut dyn $crate::p2p::binary_message::cache::CacheWriter> {
                    None
                }
            }
        };
    }
}

/// Trait for binary encoding to implement.
///
/// Binary messages could be written by a [`MessageWriter`](super::stream::MessageWriter).
/// To read binary encoding use  [`MessageReader`](super::stream::MessageReader).
pub trait BinaryMessage: Sized {
    /// Produce bytes from the struct.
    fn as_bytes(&self) -> Result<Vec<u8>, BinaryWriterError>;

    /// Create new struct from bytes.
    fn from_bytes<B: AsRef<[u8]>>(buf: B) -> Result<Self, BinaryReaderError>;

    /// Reads a new struct from the limited stream of bytes
    fn read<'a, R: BinaryRead + Unpin + Send>(
        read: &'a mut R,
    ) -> Pin<Box<dyn Future<Output = Result<Self, BinaryReaderError>> + Send + 'a>>;

    /// Reads a new struct with dynamic encoding from the stream of bytes.
    fn read_dynamic<'a, R: AsyncRead + Unpin + Send>(
        read: &'a mut R,
    ) -> Pin<Box<dyn Future<Output = Result<Self, BinaryReaderError>> + Send + 'a>>;
}

pin_project! {
    struct CachingAsyncReader<'a, A> {
        read: &'a mut A,
        cache: Vec<u8>,
    }
}

impl<'a, A> CachingAsyncReader<'a, A> {
    fn new(read: &'a mut A) -> Self {
        CachingAsyncReader {
            read,
            cache: vec![],
        }
    }
}

impl<'a, A: AsyncRead + Unpin> AsyncRead for CachingAsyncReader<'a, A> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let me = self.project();
        let len = buf.filled().len();
        let res = Pin::new(&mut *me.read).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = res {
            me.cache.extend(&buf.filled()[len..]);
        }
        res
    }
}

impl<'a, A: BinaryRead + Unpin> BinaryRead for CachingAsyncReader<'a, A> {
    fn remaining(&self) -> Result<usize, BinaryReaderError> {
        self.read.remaining()
    }
}

impl<'a, A> std::convert::AsRef<[u8]> for CachingAsyncReader<'a, A> {
    fn as_ref(&self) -> &[u8] {
        self.cache.as_ref()
    }
}

impl<T> BinaryMessage for T
where
    T: HasEncoding + cache::CachedData + DeserializeOwned + Serialize + Sized,
{
    #[inline]
    fn as_bytes(&self) -> Result<Vec<u8>, BinaryWriterError> {
        // check cache at first
        if let Some(cache) = self.cache_reader() {
            if let Some(data) = cache.get() {
                return Ok(data);
            }
        }

        // if cache not configured or empty, resolve by encoding
        binary_writer::write(self, &Self::encoding())
    }

    #[inline]
    fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Self, BinaryReaderError> {
        let bytes = bytes.as_ref();

        let value = BinaryReader::new().read(bytes, &Self::encoding())?;
        let mut myself: Self = deserialize_from_value(&value)?;
        if let Some(cache_writer) = myself.cache_writer() {
            cache_writer.put(bytes);
        }
        Ok(myself)
    }

    fn read<'a, R: BinaryRead + Unpin + Send>(
        read: &'a mut R,
    ) -> Pin<Box<dyn Future<Output = Result<Self, BinaryReaderError>> + Send + 'a>> {
        Box::pin(async move {
            if Self::has_cache() {
                let mut read = CachingAsyncReader::new(read);
                let value = BinaryAsyncReader::new()
                    .read_message(&mut read, Self::encoding())
                    .await?;
                let mut myself: Self = deserialize_from_value(&value)?;
                if let Some(cache_writer) = myself.cache_writer() {
                    cache_writer.put(read.as_ref());
                }
                Ok(myself)
            } else {
                let value = BinaryAsyncReader::new()
                    .read_message(read, Self::encoding())
                    .await?;
                Ok(deserialize_from_value(&value)?)
            }
        })
    }

    fn read_dynamic<'a, R: AsyncRead + Unpin + Send>(
        read: &'a mut R,
    ) -> Pin<Box<dyn Future<Output = Result<Self, BinaryReaderError>> + Send + 'a>> {
        Box::pin(async move {
            let myself: Self = {
                let value = {
                    BinaryAsyncReader::new()
                        .read_dynamic_message(read, Self::encoding())
                        .await?
                };
                deserialize_from_value(&value)?
            };
            Ok(myself)
        })
    }
}

/// Represents binary raw encoding received from peer node.
///
/// Difference from [`BinaryMessage`] is that it also contains [`CONTENT_LENGTH_FIELD_BYTES`] bytes
/// of information about how many bytes is the actual encoding.
pub struct BinaryChunk(Vec<u8>);

impl BinaryChunk {
    /// Create new `BinaryChunk` from input content.
    pub fn from_content(content: &[u8]) -> Result<BinaryChunk, BinaryChunkError> {
        if content.len() <= CONTENT_LENGTH_MAX {
            // add length
            let mut bytes = Vec::with_capacity(CONTENT_LENGTH_FIELD_BYTES + content.len());
            // adds MESSAGE_LENGTH_FIELD_SIZE -- 2 bytes with length of the content
            bytes.put_u16(content.len() as u16);
            // append data
            bytes.extend(content);

            Ok(BinaryChunk(bytes))
        } else {
            Err(BinaryChunkError::OverflowError)
        }
    }

    /// Gets raw data (including encoded content size)
    #[inline]
    pub fn raw(&self) -> &Vec<u8> {
        &self.0
    }

    /// Get content of the message
    #[inline]
    pub fn content(&self) -> &[u8] {
        &self.0[CONTENT_LENGTH_FIELD_BYTES..]
    }
}

/// `BinaryChunk` error
#[derive(Debug, Fail)]
pub enum BinaryChunkError {
    #[fail(display = "Overflow error")]
    OverflowError,
    #[fail(display = "Missing size information")]
    MissingSizeInformation,
    #[fail(
        display = "Incorrect content size information. expected={}, actual={}",
        expected, actual
    )]
    IncorrectSizeInformation { expected: usize, actual: usize },
}

/// Convert `Vec<u8>` into `BinaryChunk`. It is required that input `Vec<u8>`
/// contains also information about the content length in its first 2 bytes.
impl TryFrom<Vec<u8>> for BinaryChunk {
    type Error = BinaryChunkError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        if value.len() < CONTENT_LENGTH_FIELD_BYTES {
            Err(BinaryChunkError::MissingSizeInformation)
        } else if value.len() <= (CONTENT_LENGTH_MAX + CONTENT_LENGTH_FIELD_BYTES) {
            let expected_content_length =
                (&value[0..CONTENT_LENGTH_FIELD_BYTES]).get_u16() as usize;
            if (expected_content_length + CONTENT_LENGTH_FIELD_BYTES) == value.len() {
                Ok(BinaryChunk(value))
            } else {
                Err(BinaryChunkError::IncorrectSizeInformation {
                    expected: expected_content_length,
                    actual: value.len(),
                })
            }
        } else {
            Err(BinaryChunkError::OverflowError)
        }
    }
}

/// Trait for json encoding to implement.
pub trait JsonMessage {
    /// Produce JSON from the struct.
    fn as_json(&self) -> Result<String, ser::Error>;
}

impl<T> JsonMessage for T
where
    T: HasEncoding + Serialize + Sized,
{
    #[inline]
    fn as_json(&self) -> Result<String, ser::Error> {
        let mut writer = JsonWriter::new();
        writer.write(self, &Self::encoding())
    }
}

/// Message hash error
#[derive(Debug, Fail)]
pub enum MessageHashError {
    #[fail(display = "Message serialization error: {}", error)]
    SerializationError { error: BinaryWriterError },
    #[fail(display = "Error constructing hash")]
    FromBytesError { error: crypto::hash::FromBytesError },
    #[fail(display = "Blake2b digest error")]
    Blake2bError,
}

impl From<BinaryWriterError> for MessageHashError {
    fn from(error: BinaryWriterError) -> Self {
        SerializationError { error }
    }
}

impl From<crypto::hash::FromBytesError> for MessageHashError {
    fn from(error: crypto::hash::FromBytesError) -> Self {
        MessageHashError::FromBytesError { error }
    }
}

impl From<Blake2bError> for MessageHashError {
    fn from(_: Blake2bError) -> Self {
        MessageHashError::Blake2bError
    }
}

/// Trait for getting hash of the message.
pub trait MessageHash {
    fn message_hash(&self) -> Result<Hash, MessageHashError>;
    fn message_typed_hash<H>(&self) -> Result<H, MessageHashError>
    where
        H: crypto::hash::HashTrait;
}

impl<T: BinaryMessage + cache::CachedData> MessageHash for T {
    #[inline]
    fn message_hash(&self) -> Result<Hash, MessageHashError> {
        let bytes = self.as_bytes()?;
        Ok(blake2b::digest_256(&bytes)?)
    }

    #[inline]
    fn message_typed_hash<H>(&self) -> Result<H, MessageHashError>
    where
        H: crypto::hash::HashTrait,
    {
        let bytes = self.as_bytes()?;
        let digest = blake2b::digest_256(&bytes)?;
        H::try_from_bytes(&digest).map_err(|e| e.into())
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use tezos_encoding::encoding::{Encoding, Field};

    use super::cache::*;
    use super::*;
    use crate::cached_data;
    use serde::Deserialize;
    use tezos_encoding::has_encoding;

    #[test]
    fn test_binary_from_content() -> Result<(), failure::Error> {
        let chunk = BinaryChunk::from_content(&[])?.0;
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES, chunk.len());
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES, chunk.capacity());

        let chunk = BinaryChunk::from_content(&[1])?.0;
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES + 1, chunk.len());
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES + 1, chunk.capacity());

        let chunk =
            BinaryChunk::from_content(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])?.0;
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES + 15, chunk.len());
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES + 15, chunk.capacity());

        let chunk = BinaryChunk::from_content(&[1; CONTENT_LENGTH_MAX])?.0;
        assert_eq!(CONTENT_LENGTH_FIELD_BYTES + CONTENT_LENGTH_MAX, chunk.len());
        assert_eq!(
            CONTENT_LENGTH_FIELD_BYTES + CONTENT_LENGTH_MAX,
            chunk.capacity()
        );
        Ok(())
    }

    #[async_std::test]
    async fn async_read_cache() -> Result<(), failure::Error> {
        #[derive(Debug, Serialize, Deserialize)]
        #[allow(dead_code)]
        struct Msg {
            byte: u8,
            str: String,
            #[serde(skip_serializing)]
            data: BinaryDataCache,
        }
        cached_data!(Msg, data);
        has_encoding!(Msg, MSG_ENCODING, {
            Encoding::Obj(vec![
                Field::new("byte", Encoding::Uint8),
                Field::new("str", Encoding::String),
            ])
        });

        let encoded = hex::decode("00000000080102030405060708")?;
        let mut cursor = Cursor::new(encoded.clone());
        let message = Msg::read(&mut cursor).await?;
        assert!(
            matches!(message, Msg { data: BinaryDataCache { data: Some(vec) }, .. } if vec == encoded)
        );
        Ok(())
    }
}
