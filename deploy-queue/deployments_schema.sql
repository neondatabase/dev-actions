CREATE TABLE deployments (
    -- Primary key with auto-incrementing BIGSERIAL for ordering
    id BIGSERIAL PRIMARY KEY,
    
    -- Deployment metadata fields (filled at insertion)
    region VARCHAR(100) NOT NULL,
    environment VARCHAR(50) NOT NULL,
    component VARCHAR(200) NOT NULL,
    version VARCHAR(100),
    url TEXT,
    note TEXT,
    
    -- Flow process timestamps (updated after record insertion)
    start_timestamp TIMESTAMPTZ,
    finish_timestamp TIMESTAMPTZ,
    cancellation_timestamp TIMESTAMPTZ,
    cancellation_note TEXT,
    
    -- Additional useful fields
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index on region for faster queries by region
CREATE INDEX idx_deployments_region ON deployments(region);

-- Composite index on region and component for efficient blocking deployment queries
CREATE INDEX idx_deployments_region_component ON deployments(region, component);

-- Index on finish_timestamp for searching deployments in queue
CREATE INDEX idx_deployments_finish_timestamp ON deployments(finish_timestamp);

-- Index on cancellation_timestamp for searching deployments in queue
CREATE INDEX idx_deployments_cancellation_timestamp ON deployments(cancellation_timestamp);

-- Function to automatically update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Function to validate deployment state transitions
CREATE OR REPLACE FUNCTION validate_deployment_state_transition()
RETURNS TRIGGER AS $$
BEGIN
    -- Allow updates if no state-related fields are changing
    IF (OLD.start_timestamp IS NOT DISTINCT FROM NEW.start_timestamp 
        AND OLD.finish_timestamp IS NOT DISTINCT FROM NEW.finish_timestamp 
        AND OLD.cancellation_timestamp IS NOT DISTINCT FROM NEW.cancellation_timestamp) THEN
        RETURN NEW;
    END IF;

    -- Prevent any changes to finished deployments
    IF OLD.finish_timestamp IS NOT NULL THEN
        RAISE EXCEPTION 'Cannot modify deployment % - already finished at %', 
            OLD.id, OLD.finish_timestamp;
    END IF;

    -- Prevent any changes to cancelled deployments
    IF OLD.cancellation_timestamp IS NOT NULL THEN
        RAISE EXCEPTION 'Cannot modify deployment % - already cancelled at %', 
            OLD.id, OLD.cancellation_timestamp;
    END IF;

    -- Validate state transitions based on current state
    CASE
        -- QUEUED state (no timestamps set)
        WHEN OLD.start_timestamp IS NULL AND OLD.finish_timestamp IS NULL AND OLD.cancellation_timestamp IS NULL THEN
            -- Can transition to RUNNING (start_timestamp) or CANCELLED
            IF NEW.finish_timestamp IS NOT NULL THEN
                RAISE EXCEPTION 'Cannot finish deployment % - not started yet', NEW.id;
            END IF;

        -- RUNNING state (start_timestamp set, no end timestamps)
        WHEN OLD.start_timestamp IS NOT NULL AND OLD.finish_timestamp IS NULL AND OLD.cancellation_timestamp IS NULL THEN
            -- Can transition to FINISHED or CANCELLED
            -- Cannot unset start_timestamp once set
            IF NEW.start_timestamp IS NULL THEN
                RAISE EXCEPTION 'Cannot unset start_timestamp for deployment %', NEW.id;
            END IF;

        -- Invalid/unexpected state
        ELSE
            RAISE EXCEPTION 'Deployment % is in an unexpected state', OLD.id;
    END CASE;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically update updated_at on row updates
CREATE TRIGGER update_deployments_updated_at 
    BEFORE UPDATE ON deployments 
    FOR EACH ROW 
    EXECUTE FUNCTION update_updated_at_column();

-- Trigger to validate state transitions
CREATE TRIGGER validate_deployment_state_transitions
    BEFORE UPDATE ON deployments
    FOR EACH ROW
    EXECUTE FUNCTION validate_deployment_state_transition();

-- Environments table for storing environment-specific configuration
CREATE TABLE environments (
    environment VARCHAR(50) PRIMARY KEY,
    buffer_time INTEGER NOT NULL -- buffer time in minutes
);

-- Insert default environment configurations
INSERT INTO environments (environment, buffer_time) VALUES 
    ('dev', 0),      -- Development: no buffer time
    ('prod', 10);    -- Production: 10 minutes buffer time

-- Comments for documentation
COMMENT ON TABLE deployments IS 'Stores deployment records with metadata and flow process timestamps';
COMMENT ON COLUMN deployments.id IS 'Auto-incrementing primary key for ordering deployments';
COMMENT ON COLUMN deployments.region IS 'Deployment target region';
COMMENT ON COLUMN deployments.environment IS 'Deployment target environment';
COMMENT ON COLUMN deployments.component IS 'Component being deployed';
COMMENT ON COLUMN deployments.version IS 'Version of the component being deployed';
COMMENT ON COLUMN deployments.url IS 'URL to the specific GitHub Actions job';
COMMENT ON COLUMN deployments.note IS 'Info about deployment (when deploying manually)';
COMMENT ON COLUMN deployments.start_timestamp IS 'When the deployment process has started';
COMMENT ON COLUMN deployments.finish_timestamp IS 'When the deployment process was finished';
COMMENT ON COLUMN deployments.cancellation_timestamp IS 'When the deployment process was cancelled';
COMMENT ON COLUMN deployments.created_at IS 'When the record was first created (entered the queue)';
COMMENT ON COLUMN deployments.updated_at IS 'When the record was last updated';

COMMENT ON TABLE environments IS 'Stores environment-specific configuration settings';
COMMENT ON COLUMN environments.environment IS 'Environment name (e.g., dev, prod, staging)';
COMMENT ON COLUMN environments.buffer_time IS 'Buffer time in minutes for finished deployments in this environment';


