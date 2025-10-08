-- Check for blocking deployments in the same environment,cloud provider, region, and cell
--
-- This query finds deployments that are blocking the specified deployment from starting.
-- A deployment is blocked by other deployments in the same environment, cloud provider, region, and cell that:
-- 1. Have a smaller ID (were queued earlier)
-- 2. Have different or no concurrency keys (cannot run concurrently)
-- 3. Are still running (no finish_timestamp) OR finished within the buffer time
-- 4. Are not cancelled
--
-- Parameters:
-- $1: deployment_id - The ID of the deployment to check for blockers
SELECT
    blocking.id,
    blocking.environment,
    blocking.cloud_provider,
    blocking.region,
    blocking.cell_index,
    blocking.component,
    blocking.version,
    blocking.url,
    blocking.note,
    blocking.start_timestamp,
    blocking.finish_timestamp,
    blocking.cancellation_timestamp,
    blocking.cancellation_note,
    blocking.concurrency_key,
    env.buffer_time
FROM
    (
        SELECT
            *
        FROM
            deployments
        WHERE
            id = $1
    ) self
    JOIN environments env ON self.environment = env.environment
    JOIN deployments blocking ON (
        self.environment = blocking.environment
        AND self.cloud_provider = blocking.cloud_provider
        AND self.region = blocking.region
        AND self.cell_index = blocking.cell_index
        AND (
            self.concurrency_key IS NULL
            OR blocking.concurrency_key IS NULL
            OR self.concurrency_key != blocking.concurrency_key
        )
        AND blocking.id < self.id
        AND (
            blocking.finish_timestamp IS NULL
            OR blocking.finish_timestamp > NOW() - env.buffer_time
        )
        AND blocking.cancellation_timestamp IS NULL
    )
ORDER BY
    blocking.id ASC
