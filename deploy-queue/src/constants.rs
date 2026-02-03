use std::time::Duration;

pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
pub const BUSY_RETRY: Duration = Duration::from_secs(5);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15 * 60); // 15 minutes
pub const HEARTBEAT_UPDATE_TIMEOUT: Duration = Duration::from_secs(20);
pub const DEPLOYMENT_ID_LOOKUP_RETRY: Duration = Duration::from_secs(10);
pub const DEPLOYMENT_ID_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
