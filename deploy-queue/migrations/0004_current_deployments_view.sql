-- ============================================================================
-- DEPLOYMENT STATUS FUNCTION
-- ============================================================================

-- Function to determine deployment status based on timestamps and buffer time
CREATE OR REPLACE FUNCTION get_deployment_status(
    start_timestamp TIMESTAMPTZ, 
    finish_timestamp TIMESTAMPTZ, 
    cancellation_timestamp TIMESTAMPTZ, 
    buffer_time INTEGER
)
RETURNS VARCHAR(50) AS $$
BEGIN
    -- Cancelled deployments take priority
    IF cancellation_timestamp IS NOT NULL THEN
        RETURN 'cancelled';
    END IF;
    
    -- Finished deployments within buffer time
    IF finish_timestamp IS NOT NULL 
       AND cancellation_timestamp IS NULL 
       AND finish_timestamp > NOW() - INTERVAL '1 minute' * buffer_time THEN
        RETURN 'finished_buffer';
    END IF;
    
    -- Finished deployments outside buffer time
    IF finish_timestamp IS NOT NULL AND cancellation_timestamp IS NULL THEN
        RETURN 'finished';
    END IF;
    
    -- Running deployments
    IF start_timestamp IS NOT NULL 
       AND finish_timestamp IS NULL 
       AND cancellation_timestamp IS NULL THEN
        RETURN 'running';
    END IF;
    
    -- Queued deployments
    IF start_timestamp IS NULL 
       AND finish_timestamp IS NULL 
       AND cancellation_timestamp IS NULL THEN
        RETURN 'queued';
    END IF;
    
    -- Default fallback
    RETURN 'unknown';
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- PRODUCTION LATEST DEPLOYMENTS VIEW
-- ============================================================================

-- View showing the status of the latest version per region+component
CREATE VIEW prod_latest_deployments AS
SELECT 
    region,
    component,
    max_version,
    CASE 
        WHEN last_version = max_version THEN last_status
        ELSE 'pending'
    END as component_status,
    CASE 
        WHEN last_version = max_version THEN url
        ELSE ''
    END as job_url,
    created_at
FROM (
    SELECT 
        region,
        component,
        version as last_version,
        url,
        start_timestamp,
        created_at,
        get_deployment_status(start_timestamp, finish_timestamp, cancellation_timestamp, buffer_time) as last_status,
        MAX(version) OVER (PARTITION BY component) as max_version,
        ROW_NUMBER() OVER (PARTITION BY region, component ORDER BY id DESC) as rn
    FROM deployments d
    JOIN environments e ON d.environment = e.environment
    WHERE d.environment = 'prod'
      AND d.cancellation_timestamp IS NULL
) ranked_deployments
WHERE rn = 1
ORDER BY region, component;

-- ============================================================================
-- PRODUCTION CURRENT DEPLOYMENTS VIEW
-- ============================================================================

-- View showing all current (non-cancelled) production deployments
CREATE VIEW prod_current_deployments AS
SELECT 
    region,
    component,
    id as deployment_id,
    version,
    get_deployment_status(start_timestamp, finish_timestamp, cancellation_timestamp, buffer_time) as current_status,
    url,
    start_timestamp,
    finish_timestamp,
    cancellation_timestamp,
    created_at
FROM deployments d
JOIN environments e ON d.environment = e.environment
WHERE d.environment = 'prod'
  AND d.cancellation_timestamp IS NULL
ORDER BY deployment_id DESC;