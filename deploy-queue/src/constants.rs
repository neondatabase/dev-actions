use std::time::Duration;

pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
pub const BUSY_RETRY: Duration = Duration::from_secs(5);
