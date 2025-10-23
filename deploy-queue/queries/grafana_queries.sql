-- Grafana queries for deployment status and duration
--
-- Parameters:
--   $__timeFilter(created_at) - Grafana time filter
--   ${component:raw}          - Grafana variable for component name

-- ============================================================================
-- PRODUCTION LATEST DEPLOYMENTS BY COMPONENT
-- ============================================================================
-- Shows the latest deployment version status across all regions for a specific component
-- https://neonprod.grafana.net/d/urkwjlp/01-deployment-status

SELECT
    region,
    version AS "last version",
    component_status AS "status",
    job_url AS "job url"
FROM
    prod_latest_deployments
WHERE
    $__timeFilter(created_at)
    AND component = '${component:raw}'
ORDER BY
    region,
    component

-- ============================================================================
-- PRODUCTION CURRENT DEPLOYMENTS VIEW
-- ============================================================================
-- View showing all current production deployments (queued, running or buffering)
-- https://neonprod.grafana.net/d/ur2m6pj/02-current-deployments

SELECT
    deployment_id AS "id",
    region,
    component,
    current_status AS "status",
    url AS "job url",
    CASE
        WHEN current_duration IS NULL THEN NULL
        ELSE CONCAT(
            EXTRACT(hours FROM current_duration::interval)::int, 'h ',
            EXTRACT(minutes FROM current_duration::interval)::int, 'm ',
            EXTRACT(seconds FROM current_duration::interval)::int, 's'
        )
    END AS "current duration",
    CASE
        WHEN avg_duration IS NULL THEN NULL
        ELSE CONCAT(
            EXTRACT(hours FROM avg_duration::interval)::int, 'h ',
            EXTRACT(minutes FROM avg_duration::interval)::int, 'm ',
            EXTRACT(seconds FROM avg_duration::interval)::int, 's'
        )
    END AS "avg duration",
    CASE
        WHEN outlier_duration IS NULL THEN NULL
        ELSE CONCAT(
            EXTRACT(hours FROM outlier_duration::interval)::int, 'h ',
            EXTRACT(minutes FROM outlier_duration::interval)::int, 'm ',
            EXTRACT(seconds FROM outlier_duration::interval)::int, 's'
        )
    END AS "outlier duration"
FROM
    prod_current_deployments
WHERE
    $__timeFilter(created_at)


-- ============================================================================
-- PRODUCTION FINISHED DEPLOYMENTS VIEW
-- ============================================================================
-- View showing all finished (non-cancelled) production deployments
-- https://neonprod.grafana.net/d/ur2t6sf/03-finished-deployments

SELECT
    deployment_id AS "id",
    region,
    component,
    finish_timestamp AS "finished at",
    url AS "job url"
FROM
    prod_finished_deployments
WHERE
    $__timeFilter(created_at)