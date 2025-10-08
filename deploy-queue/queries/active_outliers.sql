-- Find active deployments (running, not finished/cancelled) that are taking
-- substantially longer than expected based on historical averages
--
-- An outlier is defined as a deployment where:
-- current_duration > avg_duration + 2 * stddev_duration
--
-- This threshold represents approximately the 97.5th percentile, meaning we
-- only flag deployments that are significantly slower than normal.
SELECT
    deployments.id,
    deployments.component,
    deployments.region,
    deployments.environment AS env,
    deployments.url,
    deployments.note,
    deployments.version,
    NOW() - deployments.start_timestamp AS current_duration,
    analytics.avg_duration,
    analytics.stddev_duration
FROM
    deployments
    INNER JOIN deployment_duration_analytics analytics ON deployments.component = analytics.component
    AND deployments.region = analytics.region
    AND deployments.environment = analytics.environment
WHERE
    -- Only consider running deployments
    deployments.start_timestamp IS NOT NULL
    AND deployments.finish_timestamp IS NULL
    AND deployments.cancellation_timestamp IS NULL
    -- Only flag if significantly over expected duration
    AND (NOW() - deployments.start_timestamp) > (
        analytics.avg_duration + 2 * analytics.stddev_duration
    )
ORDER BY
    deployments.id;
