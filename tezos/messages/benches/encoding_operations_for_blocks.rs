#![feature(test)]
extern crate test;

use test::Bencher;
use tezos_messages::p2p::binary_message::BinaryMessage;
use tezos_messages::p2p::encoding::prelude::*;
use tezos_encoding::de_nom::NomFrom;

#[bench]
fn serde_deserialize_nom_get_operations_for_blocks(b: &mut Bencher) {
    let message_bytes = hex::decode("0000008a006000000084ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa01ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa02ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa00ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa03").unwrap();

    b.iter(|| PeerMessageResponse::from_bytes(message_bytes.clone()).unwrap());
}

#[bench]
fn nom_deserialize_nom_get_operations_for_blocks(b: &mut Bencher) {
    let message_bytes = hex::decode("0000008a006000000084ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa01ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa02ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa00ed4197d381a4d4f56be30bf7157426671276aa187bbe0bb9484974af59e069aa03").unwrap();

    b.iter(|| PeerMessageResponse::nom_from_bytes(message_bytes.clone()).unwrap());
}

