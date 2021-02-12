use std::ops::Range;
use tezos_encoding::encoding::HasEncoding;
use tezos_encoding::generator;
use tezos_messages::p2p::encoding::advertise::*;
use tezos_messages::p2p::binary_message::BinaryMessage;

fn test_indices(_p: &generator::Path, r: Range<usize>) -> impl Iterator<Item = usize> {
    generator::range_extended(r)
}

fn test_data(p: &generator::Path, _r: Range<usize>) -> impl Iterator<Item = (Vec<u8>, bool)> {
    let path = p.as_str();
    if path.ends_with("id[0]") {
        vec![
            ("[0000:1111:2222:3333:aaaa:bbbb:cccc:dddd]:12345", true),
            ("8.8.8.8:53", true),
            ("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", false),
        ]
    } else {
        vec![
            ("[0000:1111:2222:3333:aaaa:bbbb:cccc:dddd]:12345", true),
            ("8.8.8.8:53", true),
            ("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", false),
        ]
    }.into_iter().map(|(i, v)| (i.as_bytes().to_vec(), v))
}

#[test]
fn generated_encoding_advertise() {
    let _it = generator::iter(AdvertiseMessage::encoding(), test_indices, test_data).for_each(
        |(d, v)| {
            let res = AdvertiseMessage::from_bytes(d);
            if v {
                assert!(res.is_ok(), "Successful decoding expected");
            } else {
                assert!(res.is_err(), "Unsuccessful decoding expected");
            }
        },
    );
}
