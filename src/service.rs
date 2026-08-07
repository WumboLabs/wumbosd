use std::time::Instant;

pub const API_VERSION: u32 = 1;
pub const BUS_NAME: &str = "org.wumbos.wumbosd";
pub const INTERFACE_NAME: &str = "org.wumbos.wumbosd1";
pub const OBJECT_PATH: &str = "/org/wumbos/wumbosd";
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SERVICE_STATE: &str = "ready";
pub const PING_RESPONSE: &str = "pong";

pub struct FoundationService {
    started_at: Instant,
}

impl FoundationService {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }

    fn elapsed_milliseconds(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
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
        self.elapsed_milliseconds()
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
        let service = FoundationService::new();
        let first = service.elapsed_milliseconds();
        let second = service.elapsed_milliseconds();

        assert!(second >= first);
    }
}
