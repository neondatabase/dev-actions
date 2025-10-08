-- Add deployment duration analytics materialized view
-- Provides averages and standard deviation of deployment durations per component per region
-- Includes trigger for automatic refresh on deployment finish

-- Materialized view for deployment duration statistics
CREATE MATERIALIZED VIEW deployment_duration_analytics AS
SELECT
    component,
    region,
    environment,
    COUNT(*) as deployment_count,
    AVG(finish_timestamp - start_timestamp) as avg_duration,
    STDDEV(EXTRACT(EPOCH FROM (finish_timestamp - start_timestamp))) * INTERVAL '1 second' as stddev_duration
FROM (
    SELECT
        component,
        region,
        environment,
        start_timestamp,
        finish_timestamp,
        ROW_NUMBER() OVER (
            PARTITION BY component, region, environment
            ORDER BY id DESC
        ) as row_number
    FROM deployments
    WHERE
        start_timestamp IS NOT NULL
        AND finish_timestamp IS NOT NULL
        AND cancellation_timestamp IS NULL
        AND created_at >= NOW() - INTERVAL '3 months'
) recent_deployments
WHERE row_number <= 100
GROUP BY component, region, environment;

-- Indexes for efficient querying
CREATE INDEX idx_deployment_duration_analytics_component ON deployment_duration_analytics(component);
CREATE INDEX idx_deployment_duration_analytics_region ON deployment_duration_analytics(region);
CREATE INDEX idx_deployment_duration_analytics_component_region ON deployment_duration_analytics(component, region);

-- Trigger function to handle deployment finish
CREATE OR REPLACE FUNCTION on_deployment_finished()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.finish_timestamp IS NULL AND NEW.finish_timestamp IS NOT NULL THEN
        REFRESH MATERIALIZED VIEW deployment_duration_analytics;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically refresh on deployment finish
CREATE TRIGGER refresh_analytics_on_deployment_finish
    AFTER UPDATE ON deployments
    FOR EACH ROW
    EXECUTE FUNCTION on_deployment_finished();
