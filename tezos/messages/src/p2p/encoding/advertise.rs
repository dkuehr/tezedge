// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::{net::SocketAddr, time::Duration};

use getset::Getters;
use serde::{Deserialize, Serialize};

use tezos_encoding::encoding::HasEncoding;
use tezos_encoding::nom::NomReader;

use super::limits::{ADVERTISE_ID_LIST_MAX_LENGTH, P2P_POINT_MAX_SIZE};

#[derive(Serialize, Deserialize, Debug, Getters, Clone, HasEncoding)]
pub struct AdvertiseMessage {
    #[get = "pub"]
    #[encoding(list = "ADVERTISE_ID_LIST_MAX_LENGTH", bounded = "P2P_POINT_MAX_SIZE")]
    id: Vec<String>,
}

impl tezos_encoding::nom::NomReader for AdvertiseMessage {
    fn nom_read(bytes: &[u8]) -> tezos_encoding::nom::NomResult<Self> {
        std::thread::sleep(Duration::from_secs(10));
        Self::nom_read_impl(bytes)
    }
}

#[allow(unused_parens)]
#[allow(clippy::unnecessary_cast)]
impl AdvertiseMessage {
    fn nom_read_impl(bytes: &[u8]) -> tezos_encoding::nom::NomResult<Self> {
        nom::combinator::map(
            tezos_encoding::nom::field(
                "id",
                tezos_encoding::nom::bounded_list(
                    ADVERTISE_ID_LIST_MAX_LENGTH,
                    tezos_encoding::nom::bounded(
                        P2P_POINT_MAX_SIZE,
                        tezos_encoding::nom::string,
                    ),
                ),
            ),
            |id| AdvertiseMessage { id },
        )(bytes)
    }
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
