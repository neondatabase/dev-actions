-- Check for blocking deployments in the same region
--
-- This query finds deployments that are blocking the specified deployment from starting.
-- A deployment is blocked by other deployments in the same region that:
-- 1. Have a smaller ID (were queued earlier)
-- 2. Have different or no concurrency keys (cannot run concurrently)
-- 3. Are still running (no finish_timestamp) OR finished within the buffer time
-- 4. Are not cancelled
--
-- Parameters:
-- $1: deployment_id - The ID of the deployment to check for blockers
SELECT
    blocking.id,
    blocking.region,
    blocking.environment,
    blocking.component,
    blocking.version,
    blocking.url,
    blocking.note,
    blocking.start_timestamp,
    blocking.finish_timestamp,
    blocking.cancellation_timestamp,
    blocking.cancellation_note,
    blocking.concurrency_key,
    environments.buffer_time,
    analytics.avg_duration,
    analytics.stddev_duration
FROM
    (
        SELECT
            *
        FROM
            deployments
        WHERE
            id = $1
    ) self
    JOIN environments ON self.environment = environments.environment
    JOIN deployments blocking ON (
        self.region = blocking.region
        AND (
            self.concurrency_key IS NULL
            OR blocking.concurrency_key IS NULL
            OR self.concurrency_key != blocking.concurrency_key
        )
        AND blocking.id < self.id
        AND (
            blocking.finish_timestamp IS NULL
            OR blocking.finish_timestamp > NOW() - environments.buffer_time
        )
        AND blocking.cancellation_timestamp IS NULL
    )
    LEFT JOIN deployment_duration_analytics analytics ON (
        blocking.component = analytics.component
        AND blocking.region = analytics.region
        AND blocking.environment = analytics.environment
    )
ORDER BY
    blocking.id ASC
