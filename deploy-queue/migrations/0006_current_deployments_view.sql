-- ============================================================================
-- DEPLOYMENT STATUS TYPE & FUNCTION
-- ============================================================================
-- Enum representing the lifecycle states of a deployment:
--   'pending'   - Deployment version exists but hasn't been deployed to this region yet
--   'queued'    - Deployment has been queued but not yet started
--   'running'   - Deployment is currently in progress
--   'buffering' - Deployment has finished but is within the buffer time window
--   'finished'  - Deployment has finished and buffer time has elapsed
--   'cancelled' - Deployment was cancelled
CREATE TYPE deployment_status AS ENUM (
    'pending',
    'queued',
    'running',
    'buffering',
    'finished',
    'cancelled'
);

-- Function to determine deployment status based on timestamps and buffer time
CREATE
OR REPLACE FUNCTION get_deployment_status(
    start_timestamp TIMESTAMPTZ,
    finish_timestamp TIMESTAMPTZ,
    cancellation_timestamp TIMESTAMPTZ,
    buffer_time INTERVAL
) RETURNS deployment_status AS
$$
BEGIN
-- Cancelled deployments take priority
IF cancellation_timestamp IS NOT NULL THEN RETURN 'cancelled';

END IF;

-- Finished deployments within buffer time
IF finish_timestamp IS NOT NULL
AND finish_timestamp > NOW() - buffer_time THEN RETURN 'buffering';

END IF;

-- Finished deployments outside buffer time
IF finish_timestamp IS NOT NULL THEN RETURN 'finished';

END IF;

-- Running deployments
IF start_timestamp IS NOT NULL THEN RETURN 'running';

END IF;

-- Queued deployments
RETURN 'queued';

END;

$$
LANGUAGE plpgsql;

-- ============================================================================
-- PRODUCTION LATEST DEPLOYMENTS VIEW
-- ============================================================================
-- View showing the status of the latest version per region+component
CREATE VIEW prod_latest_deployments AS WITH latest_versions AS (
    SELECT
        component,
        environment,
        version
    FROM
        (
            SELECT
                component,
                environment,
                version,
                ROW_NUMBER() OVER (
                    PARTITION BY component,
                    environment
                    ORDER BY
                        id DESC
                ) AS row_number
            FROM
                deployments
            WHERE
                environment = 'prod'
                AND cancellation_timestamp IS NULL
                AND version IS NOT NULL
        ) ranked
    WHERE
        row_number = 1
),
latest_deployments AS (
    SELECT
        deployment.environment,
        deployment.cloud_provider,
        deployment.region,
        deployment.cell_index,
        deployment.component,
        deployment.version AS region_version,
        deployment.url,
        deployment.start_timestamp,
        deployment.finish_timestamp,
        deployment.cancellation_timestamp,
        deployment.created_at,
        environment.buffer_time,
        ROW_NUMBER() OVER (
            PARTITION BY deployment.environment,
            deployment.cloud_provider,
            deployment.region,
            deployment.cell_index,
            deployment.component
            ORDER BY
                deployment.id DESC
        ) AS row_number
    FROM
        deployments deployment
        JOIN environments environment ON deployment.environment = environment.environment
    WHERE
        deployment.environment = 'prod'
        AND deployment.cancellation_timestamp IS NULL
        AND deployment.version IS NOT NULL
)
SELECT
    deployment.environment,
    deployment.cloud_provider,
    deployment.region,
    deployment.cell_index,
    deployment.component,
    latest_versions.version,
    CASE
        WHEN deployment.region_version = latest_versions.version THEN get_deployment_status(
            deployment.start_timestamp,
            deployment.finish_timestamp,
            deployment.cancellation_timestamp,
            deployment.buffer_time
        )
        ELSE 'pending'
    END AS component_status,
    CASE
        WHEN deployment.region_version = latest_versions.version THEN deployment.url
        ELSE ''
    END AS job_url,
    CASE
        WHEN deployment.region_version = latest_versions.version THEN deployment.created_at
        ELSE NULL
    END AS created_at
FROM
    latest_deployments deployment
    JOIN latest_versions ON deployment.component = latest_versions.component
    AND deployment.environment = latest_versions.environment
WHERE
    deployment.row_number = 1
ORDER BY
    deployment.environment,
    deployment.cloud_provider,
    deployment.region,
    deployment.cell_index,
    deployment.component;

-- ============================================================================
-- PRODUCTION CURRENT DEPLOYMENTS VIEW
-- ============================================================================
-- View showing all current production deployments
CREATE VIEW prod_current_deployments AS
SELECT
    *
FROM
    (
        SELECT
            deployment.environment,
            deployment.cloud_provider,
            deployment.region,
            deployment.cell_index,
            deployment.component,
            deployment.id AS deployment_id,
            deployment.version,
            get_deployment_status(
                deployment.start_timestamp,
                deployment.finish_timestamp,
                deployment.cancellation_timestamp,
                environment.buffer_time
            ) AS current_status,
            deployment.url,
            deployment.start_timestamp,
            deployment.finish_timestamp,
            deployment.cancellation_timestamp,
            deployment.created_at,
            NOW() - deployment.start_timestamp AS current_duration,
            analytics.avg_duration,
            analytics.avg_duration + 2 * analytics.stddev_duration AS outlier_duration
        FROM
            deployments deployment
            LEFT JOIN deployment_duration_analytics analytics ON deployment.environment = analytics.environment
            AND deployment.cloud_provider = analytics.cloud_provider
            AND deployment.region = analytics.region
            AND deployment.cell_index = analytics.cell_index
            AND deployment.component = analytics.component
            LEFT JOIN environments environment ON deployment.environment = environment.environment
        WHERE
            deployment.environment = 'prod'
            AND deployment.cancellation_timestamp IS NULL
    ) AS current_deployments
WHERE
    current_status NOT IN ('finished')
ORDER BY
    deployment_id ASC;

-- ============================================================================
-- PRODUCTION FINISHED DEPLOYMENTS VIEW
-- ============================================================================
-- View showing all finished (non-cancelled) production deployments
CREATE VIEW prod_finished_deployments AS
SELECT
    *
FROM
    (
        SELECT
            deployment.environment,
            deployment.cloud_provider,
            deployment.region,
            deployment.cell_index,
            deployment.component,
            deployment.id AS deployment_id,
            deployment.version,
            get_deployment_status(
                deployment.start_timestamp,
                deployment.finish_timestamp,
                deployment.cancellation_timestamp,
                environment.buffer_time
            ) AS current_status,
            deployment.url,
            deployment.start_timestamp,
            deployment.finish_timestamp,
            deployment.created_at
        FROM
            deployments deployment
            LEFT JOIN environments environment ON deployment.environment = environment.environment
        WHERE
            deployment.environment = 'prod'
            AND deployment.cancellation_timestamp IS NULL
            AND deployment.finish_timestamp IS NOT NULL
    ) AS finished_deployments
WHERE
    current_status IN ('finished')
ORDER BY
    deployment_id ASC;
