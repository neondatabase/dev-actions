-- Add cells materialized view
-- Provides distinct cells (environment + cloud_provider + region + cell_index combinations)
-- Includes trigger for automatic refresh on deployment insert
-- Materialized view for distinct cells
CREATE MATERIALIZED VIEW cells AS
SELECT
    DISTINCT environment,
    cloud_provider,
    region,
    cell_index
FROM
    deployments
ORDER BY
    environment,
    cloud_provider,
    region,
    cell_index;

-- Unique index required for concurrent refresh
CREATE UNIQUE INDEX idx_cells_unique ON cells(environment, cloud_provider, region, cell_index);

-- Index for efficient querying by environment
CREATE INDEX idx_cells_environment ON cells(environment);

-- Trigger function to handle deployment insert
CREATE
OR REPLACE FUNCTION on_deployment_inserted() RETURNS TRIGGER AS
$$
BEGIN
-- Refresh the materialized view when a new deployment is inserted
-- Using CONCURRENTLY to avoid blocking concurrent reads
REFRESH MATERIALIZED VIEW CONCURRENTLY cells;

RETURN NEW;

END;

$$
LANGUAGE plpgsql;

-- Trigger to automatically refresh on deployment insert
CREATE TRIGGER refresh_cells_on_deployment_insert
AFTER
INSERT
    ON deployments FOR EACH STATEMENT EXECUTE FUNCTION on_deployment_inserted();
