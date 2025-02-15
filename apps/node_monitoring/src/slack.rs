// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use slog::{error, info, Logger};

#[derive(Clone)]
pub struct SlackServer {
    monitor_channel_url: String,
    auth_token: String,
    channel: String,
    log: Logger,
}

impl SlackServer {
    pub fn new(
        monitor_channel_url: String,
        auth_token: String,
        channel: String,
        log: Logger,
    ) -> Self {
        Self {
            monitor_channel_url,
            auth_token,
            channel,
            log,
        }
    }

    pub async fn send_message(&self, text: &str) {
        let client = reqwest::Client::new();

        let mut map = HashMap::new();
        map.insert("text", text);

        let res = client
            .post(&self.monitor_channel_url)
            .json(&map)
            .send()
            .await;

        match res {
            Ok(_) => info!(self.log, "Slack message sent: {}", text),
            Err(e) => error!(self.log, "Slack message error: {:?}", e),
        }
    }
}
