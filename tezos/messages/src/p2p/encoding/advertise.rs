// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::net::SocketAddr;

use getset::Getters;
use serde::Serialize;

use tezos_encoding::enc::BinWriter;
use tezos_encoding::encoding::HasEncoding;
use tezos_encoding::nom::NomReader;

use super::limits::{ADVERTISE_ID_LIST_MAX_LENGTH, P2P_POINT_MAX_SIZE};

#[derive(
    Serialize,
    Debug,
    Getters,
    Clone,
    HasEncoding,
    NomReader,
    BinWriter,
    tezos_encoding::generator::Generated,
)]
pub struct AdvertiseMessage {
    #[get = "pub"]
    #[encoding(list = "ADVERTISE_ID_LIST_MAX_LENGTH", bounded = "P2P_POINT_MAX_SIZE")]
    id: Vec<String>,
}

impl AdvertiseMessage {
    pub fn new(addresses: &[SocketAddr]) -> Self {
        Self {
            id: addresses
                .iter()
                .map(|address| format!("{}", address))
                .collect(),
        }
    }
}
