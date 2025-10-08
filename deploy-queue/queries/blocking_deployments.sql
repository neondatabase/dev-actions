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
    d2.id,
    d2.environment,
    d2.cloud_provider,
    d2.region,
    d2.cell_index,
    d2.component,
    d2.version,
    d2.url,
    d2.note,
    d2.start_timestamp,
    d2.finish_timestamp,
    d2.cancellation_timestamp,
    d2.cancellation_note,
    d2.concurrency_key,
    e.buffer_time
FROM
    (
        SELECT
            *
        FROM
            deployments
        WHERE
            id = $1
    ) d1
    JOIN environments e ON d1.environment = e.environment
    JOIN deployments d2 ON (
        d1.environment = d2.environment
        AND d1.cloud_provider = d2.cloud_provider
        AND d1.region = d2.region
        AND d1.cell_index = d2.cell_index
        AND (
            d1.concurrency_key IS NULL
            OR d2.concurrency_key IS NULL
            OR d1.concurrency_key != d2.concurrency_key
        )
        AND d2.id < d1.id
        AND (
            d2.finish_timestamp IS NULL
            OR d2.finish_timestamp > NOW() - INTERVAL '1 minute' * e.buffer_time
        )
        AND d2.cancellation_timestamp IS NULL
    )
ORDER BY
    d2.id ASC
