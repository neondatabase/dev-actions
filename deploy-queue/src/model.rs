use anyhow::{Context, Result};
use serde::Serialize;
use time::{Duration, OffsetDateTime};

use crate::{cli::StartDeployment, util::duration::DurationExt};

#[derive(Default, Debug, Clone, Serialize)]
pub struct Cell {
    pub environment: String,
    pub cloud_provider: String,
    pub region: String,
    pub index: i32,
}

// We don't read all of the fields
#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct Deployment {
    pub id: i64,
    pub cell: Cell,
    pub component: String,
    pub version: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub start_timestamp: Option<OffsetDateTime>,
    pub finish_timestamp: Option<OffsetDateTime>,
    pub cancellation_timestamp: Option<OffsetDateTime>,
    pub cancellation_note: Option<String>,
    pub concurrency_key: Option<String>,
    pub buffer_time: Duration,
}

/// Minimal view of a deployment for stale-heartbeat checks
pub struct StaleHeartbeatDeployment {
    pub id: i64,
    pub component: String,
    pub version: Option<String>,
    pub heartbeat_timestamp: OffsetDateTime,
    pub time_since_heartbeat: Duration,
}

impl Deployment {
    /// Generate a compact summary of this deployment's information
    pub fn summary(&self) -> String {
        let state = DeploymentState::from_timestamps(
            self.start_timestamp,
            self.finish_timestamp,
            self.cancellation_timestamp,
        );
        let state_verb = state.state_verb();

        let mut summary = format!(
            "{} {} {}(@{})",
            self.id,
            state_verb,
            self.component,
            self.version.as_deref().unwrap_or("unknown")
        );

        if let Some(ref note) = self.note {
            summary.push_str(&format!(": ({})", note));
        }

        if let Some(ref url) = self.url {
            summary.push_str(&format!(" ({})", url));
        }

        summary
    }
}

impl From<StartDeployment> for Deployment {
    fn from(
        StartDeployment {
            environment,
            cloud_provider,
            region,
            cell_index,
            component,
            version,
            url,
            note,
            concurrency_key,
        }: StartDeployment,
    ) -> Self {
        Deployment {
            cell: Cell {
                environment: environment.to_string(),
                cloud_provider,
                region,
                index: cell_index,
            },
            component,
            version,
            url,
            note,
            concurrency_key,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeploymentState {
    Queued,
    Running,
    Finished,
    Cancelled,
}

impl std::fmt::Display for DeploymentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentState::Queued => write!(f, "queued"),
            DeploymentState::Running => write!(f, "running"),
            DeploymentState::Finished => write!(f, "finished"),
            DeploymentState::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl DeploymentState {
    pub fn from_timestamps(
        start: Option<OffsetDateTime>,
        finish: Option<OffsetDateTime>,
        cancel: Option<OffsetDateTime>,
    ) -> Self {
        match (start, finish, cancel) {
            (_, _, Some(_)) => DeploymentState::Cancelled,
            (_, Some(_), None) => DeploymentState::Finished,
            (Some(_), None, None) => DeploymentState::Running,
            (None, None, None) => DeploymentState::Queued,
        }
    }

    fn state_verb(&self) -> &'static str {
        match self {
            DeploymentState::Queued => "queued",
            DeploymentState::Running => "deploying",
            DeploymentState::Finished => "deployed",
            DeploymentState::Cancelled => "cancelled",
        }
    }
}

/// Represents a blocking deployment with analytics data for ETA calculation
#[derive(Debug, Clone)]
pub struct BlockingDeployment {
    pub deployment: Deployment,
    pub avg_duration: Option<Duration>,
    pub stddev_duration: Option<Duration>,
}

impl BlockingDeployment {
    /// Calculate the remaining time for this blocking deployment
    /// Returns None if no analytics data is available
    pub fn remaining_time(&self) -> Result<(Option<Duration>, Duration)> {
        let state = DeploymentState::from_timestamps(
            self.deployment.start_timestamp,
            self.deployment.finish_timestamp,
            self.deployment.cancellation_timestamp,
        );

        match state {
            DeploymentState::Queued => {
                // Hasn't started yet, full duration expected
                Ok((self.avg_duration, self.deployment.buffer_time))
            }
            DeploymentState::Running => {
                // Started but not finished, calculate remaining time
                if let (Some(start_time), Some(avg_duration)) =
                    (self.deployment.start_timestamp, self.avg_duration)
                {
                    let now = OffsetDateTime::now_utc();
                    let elapsed = now - start_time;
                    let remaining = avg_duration - elapsed;
                    // Return at least 0, even if overdue
                    Ok((
                        Some(remaining.max(Duration::ZERO)),
                        self.deployment.buffer_time,
                    ))
                } else {
                    Ok((None, self.deployment.buffer_time))
                }
            }
            DeploymentState::Finished => {
                let finish_time = self.deployment.finish_timestamp.with_context(|| {
                    format!(
                        "Finish timestamp is required for finished deployment {}",
                        self.deployment.id
                    )
                })?;
                let now = OffsetDateTime::now_utc();
                let time_since_finish = now - finish_time;
                let remaining = self.deployment.buffer_time - time_since_finish;
                Ok((None, remaining.max(Duration::ZERO)))
            }
            DeploymentState::Cancelled => {
                anyhow::bail!("Cancelled deployment {} is blocking", self.deployment.id);
            }
        }
    }

    /// Generate a compact summary with ETA information
    pub fn summary(&self) -> Result<String> {
        let state = DeploymentState::from_timestamps(
            self.deployment.start_timestamp,
            self.deployment.finish_timestamp,
            self.deployment.cancellation_timestamp,
        );
        let state_verb = state.state_verb();

        let mut summary = format!(
            "{} {} {}(@{})",
            self.deployment.id,
            state_verb,
            self.deployment.component,
            self.deployment.version.as_deref().unwrap_or("unknown")
        );

        // Add remaining time information
        let (deployment_time, buffer_time) = self.remaining_time().with_context(|| {
            format!(
                "Failed to calculate remaining time for deployment {}",
                self.deployment.id
            )
        })?;

        match (deployment_time, state) {
            (Some(deployment_time), _) => {
                // Have analytics data for deployment time
                let total_time = deployment_time + buffer_time;
                if total_time > Duration::ZERO {
                    summary.push_str(&format!(": ~{} remaining", total_time.format_human()));
                    if buffer_time > Duration::ZERO {
                        summary.push_str(&format!(
                            " (includes ~{} buffer)",
                            buffer_time.format_human()
                        ));
                    }
                } else if state == DeploymentState::Running {
                    // Running but overdue
                    summary.push_str(": overdue");
                    if buffer_time > Duration::ZERO {
                        summary.push_str(&format!(
                            ", ~{} buffer remaining",
                            buffer_time.format_human()
                        ));
                    }
                }
            }
            (None, DeploymentState::Finished) => {
                // Finished - only show buffer time if present
                if buffer_time > Duration::ZERO {
                    summary.push_str(&format!(
                        ": ~{} buffer remaining",
                        buffer_time.format_human()
                    ));
                }
            }
            (None, DeploymentState::Queued | DeploymentState::Running) => {
                // No analytics data for queued/running deployment
                summary.push_str(": unknown deployment time");
                if buffer_time > Duration::ZERO {
                    summary.push_str(&format!(", ~{} buffer", buffer_time.format_human()));
                }
            }
            _ => {
                // Any other case
            }
        }

        if let Some(ref note) = self.deployment.note {
            summary.push_str(&format!(" ({})", note));
        }

        if let Some(ref url) = self.deployment.url {
            summary.push_str(&format!(" ({})", url));
        }

        Ok(summary)
    }
}

/// Represents a deployment that is taking substantially longer than expected
#[derive(Debug, Clone, Serialize)]
pub struct OutlierDeployment {
    pub id: i64,
    pub env: String,
    pub cloud_provider: String,
    pub region: String,
    pub cell_index: i32,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(serialize_with = "serialize_duration_humantime")]
    pub current_duration: Duration,
    #[serde(serialize_with = "serialize_duration_humantime")]
    pub avg_duration: Duration,
    #[serde(serialize_with = "serialize_duration_humantime")]
    pub stddev_duration: Duration,
}

/// Convert time::Duration to std::time::Duration for humantime serialization
/// Rounds to whole seconds for cleaner output
fn serialize_duration_humantime<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let std_duration = duration
        .to_std_duration()
        .map_err(|e| serde::ser::Error::custom(e.to_string()))?;
    humantime_serde::serialize(&std_duration, serializer)
}
