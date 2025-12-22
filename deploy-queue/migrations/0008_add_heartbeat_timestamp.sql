-- Add heartbeat_timestamp column to deployments table
-- This field tracks the last heartbeat timestamp for deployment liveness monitoring
ALTER TABLE
    deployments
ADD
    COLUMN heartbeat_timestamp TIMESTAMPTZ;

-- Add column comment
COMMENT ON COLUMN deployments.heartbeat_timestamp IS 'Last heartbeat timestamp for tracking deployment liveness';
