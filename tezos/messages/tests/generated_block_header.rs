use std::ops::Range;
use tezos_encoding::encoding::HasEncoding;
use tezos_encoding::generator::*;
use tezos_messages::p2p::encoding::block_header::*;
use tezos_messages::p2p::binary_message::BinaryMessage;

fn test_indices(_p: &Path, r: &Constraint) -> impl Iterator<Item = usize> {
    range_extended(r)
}

fn test_data(p: &Path, r: &Constraint) -> impl Iterator<Item = (Vec<u8>, bool)> {
    let r = match p.kind() {
        ItemKind::Index(i) if *i != 0 => Constraint::new(Some(0), Some(0)),
        _ => r.clone(),
    };
    let r = if r.upper.is_none() { Constraint::new(r.lower, Some(10)) } else { r };
    range_extended(&r).map(move |s| ((0..(s as u8)).collect::<Vec<_>>(), r.contains(&s)))
}

#[test]
fn generated_encoding_block_header() {
    let _it = iter(BlockHeader::encoding(), test_indices, test_data).for_each(
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
