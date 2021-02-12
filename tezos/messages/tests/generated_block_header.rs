use std::ops::Range;
use tezos_encoding::encoding::HasEncoding;
use tezos_encoding::generator;
use tezos_messages::p2p::encoding::block_header::*;
use tezos_messages::p2p::binary_message::BinaryMessage;

fn test_indices(_p: &generator::Path, r: Range<usize>) -> impl Iterator<Item = usize> {
    generator::range_extended(r)
}

fn test_data(_p: &generator::Path, r: Range<usize>) -> Vec<(Vec<u8>, bool)> {
    let mut data = Vec::new();
    data.resize(r.end, 0);
    vec![(data, true)]
}

#[test]
fn generated_encoding_block_header() {
    let _it = generator::iter(BlockHeader::encoding(), test_indices, test_data).for_each(
        |(d, v)| {
            let res = BlockHeader::from_bytes(d);
            if v {
                assert!(res.is_ok(), "Successful decoding expected");
            } else {
                assert!(res.is_err(), "Unsuccessful decoding expected");
            }
        },
    );
}
