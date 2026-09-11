// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::time::Instant;

pub const API_VERSION: u32 = 1;
pub const BUS_NAME: &str = "org.wumbos.wumbosd";
pub const INTERFACE_NAME: &str = "org.wumbos.wumbosd1";
pub const OBJECT_PATH: &str = "/org/wumbos/wumbosd";
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SERVICE_STATE: &str = "ready";
pub const PING_RESPONSE: &str = "pong";

#[derive(Clone)]
pub struct FoundationState {
    started_at: Instant,
}

impl FoundationState {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }

    pub fn elapsed_milliseconds(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

pub struct FoundationService {
    state: FoundationState,
}

impl FoundationService {
    pub fn new(state: FoundationState) -> Self {
        Self { state }
    }
}

#[zbus::interface(name = "org.wumbos.wumbosd1")]
impl FoundationService {
    fn ping(&self) -> &'static str {
        PING_RESPONSE
    }

    #[zbus(property)]
    fn api_version(&self) -> u32 {
        API_VERSION
    }

    #[zbus(property)]
    fn package_version(&self) -> &'static str {
        PACKAGE_VERSION
    }

    #[zbus(property)]
    fn status(&self) -> &'static str {
        SERVICE_STATE
    }

    #[zbus(property)]
    fn uptime_milliseconds(&self) -> u64 {
        self.state.elapsed_milliseconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_is_one() {
        assert_eq!(API_VERSION, 1);
    }

    #[test]
    fn package_version_is_nonempty() {
        assert!(!PACKAGE_VERSION.is_empty());
    }

    #[test]
    fn initial_status_is_ready() {
        assert_eq!(SERVICE_STATE, "ready");
    }

    #[test]
    fn uptime_is_nondecreasing() {
        let state = FoundationState::new();
        let first = state.elapsed_milliseconds();
        let second = state.elapsed_milliseconds();

        assert!(second >= first);
    }
}
