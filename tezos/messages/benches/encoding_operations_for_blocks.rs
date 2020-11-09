#![feature(test)]
extern crate test;

use test::Bencher;
use tezos_messages::p2p::binary_message::BinaryMessage;
use tezos_messages::p2p::encoding::prelude::*;
use tezos_encoding::de_nom::NomFrom;

#[bench]
fn serde_deserialize_get_operations_for_blocks(b: &mut Bencher) {
    let message_bytes = hex::decode("0000008a006000000084ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa01ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa02ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa00ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa03").unwrap();

    b.iter(|| PeerMessageResponse::from_bytes(message_bytes.clone()).unwrap());
}

#[bench]
fn nom_deserialize_get_operations_for_blocks(b: &mut Bencher) {
    let message_bytes = hex::decode("0000008a006000000084ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa01ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa02ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa00ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa03").unwrap();

    b.iter(|| PeerMessageResponse::nom_from_bytes(message_bytes.clone()).unwrap());
}

#[bench]
fn serde_deserialize_operations_for_blocks(b: &mut Bencher) {
    let message_bytes = hex::decode("000000a80061ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa01f00ffe7601035ca2892f983c10203656479cfd2f8a4ea656f300cd9d68f74aa62587f00f7c09f7c4d76ace86e1a7e1c7dc0a0c7edcaa8b284949320081131976a87760c30032bc1d3a28df9a67b363aa1638f807214bb8987e5f9c0abcbd69531facffd1c80a37f18e2562ae14388716247be0d4e451d72ce38d1d4a30f92d2f6ef95b4919").unwrap();

    b.iter(|| PeerMessageResponse::from_bytes(message_bytes.clone()).unwrap());
}

#[bench]
fn nom_deserialize_operations_for_blocks(b: &mut Bencher) {
    let message_bytes = hex::decode("000000a80061ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa01f00ffe7601035ca2892f983c10203656479cfd2f8a4ea656f300cd9d68f74aa62587f00f7c09f7c4d76ace86e1a7e1c7dc0a0c7edcaa8b284949320081131976a87760c30032bc1d3a28df9a67b363aa1638f807214bb8987e5f9c0abcbd69531facffd1c80a37f18e2562ae14388716247be0d4e451d72ce38d1d4a30f92d2f6ef95b4919").unwrap();

    b.iter(|| PeerMessageResponse::nom_from_bytes(message_bytes.clone()).unwrap());
}

