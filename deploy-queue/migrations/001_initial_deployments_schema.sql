-- Initial deployment queue database schema
-- Contains deployments table, environments configuration, and validation triggers

-- ============================================================================
-- DEPLOYMENTS TABLE
-- ============================================================================

-- Main deployments table with metadata and flow process timestamps
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

-- Indexes for efficient queries
CREATE INDEX idx_deployments_region ON deployments(region);
CREATE INDEX idx_deployments_region_component ON deployments(region, component);
CREATE INDEX idx_deployments_finish_timestamp ON deployments(finish_timestamp);
CREATE INDEX idx_deployments_cancellation_timestamp ON deployments(cancellation_timestamp);

-- ============================================================================
-- ENVIRONMENTS TABLE
-- ============================================================================

-- Environment-specific configuration settings
CREATE TABLE environments (
    environment VARCHAR(50) PRIMARY KEY,
    buffer_time INTEGER NOT NULL -- buffer time in minutes
);

-- Insert default environment configurations
INSERT INTO environments (environment, buffer_time) VALUES 
    ('dev', 0),      -- Development: no buffer time
    ('prod', 10);    -- Production: 10 minutes buffer time

-- ============================================================================
-- FUNCTIONS AND TRIGGERS
-- ============================================================================

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
    -- Prevent changes to immutable fields
    IF (OLD.id IS DISTINCT FROM NEW.id 
        OR OLD.region IS DISTINCT FROM NEW.region 
        OR OLD.environment IS DISTINCT FROM NEW.environment 
        OR OLD.component IS DISTINCT FROM NEW.component 
        OR OLD.version IS DISTINCT FROM NEW.version 
        OR OLD.url IS DISTINCT FROM NEW.url 
        OR OLD.note IS DISTINCT FROM NEW.note) THEN
        RAISE EXCEPTION 'Cannot modify immutable fields (id, region, environment, component, version, url, note) for deployment %', OLD.id;
    END IF;

    -- Prevent both finish_timestamp and cancellation_timestamp from being set
    IF NEW.finish_timestamp IS NOT NULL AND NEW.cancellation_timestamp IS NOT NULL THEN
        RAISE EXCEPTION 'Deployment % cannot be both finished and cancelled', NEW.id;
    END IF;

    -- Prevent any changes to finished deployments
    IF OLD.finish_timestamp IS NOT NULL THEN
        RAISE EXCEPTION 'Cannot modify deployment % - already finished at %', 
            OLD.id, OLD.finish_timestamp;
    END IF;

    -- Prevent setting cancellation_note without cancelling
    IF (OLD.cancellation_note IS DISTINCT FROM NEW.cancellation_note) 
        AND (OLD.cancellation_timestamp IS NOT DISTINCT FROM NEW.cancellation_timestamp) THEN
        RAISE EXCEPTION 'Cannot set cancellation_note without cancelling deployment %', NEW.id;
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

-- Apply triggers to deployments table
CREATE TRIGGER update_deployments_updated_at 
    BEFORE UPDATE ON deployments 
    FOR EACH ROW 
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER validate_deployment_state_transitions
    BEFORE UPDATE ON deployments
    FOR EACH ROW
    EXECUTE FUNCTION validate_deployment_state_transition();

-- ============================================================================
-- DOCUMENTATION
-- ============================================================================

-- Table comments
COMMENT ON TABLE deployments IS 'Stores deployment records with metadata and flow process timestamps';
COMMENT ON TABLE environments IS 'Stores environment-specific configuration settings';

-- Column comments for deployments table
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

-- Column comments for environments table
COMMENT ON COLUMN environments.environment IS 'Environment name (e.g., dev, prod, staging)';
COMMENT ON COLUMN environments.buffer_time IS 'Buffer time in minutes for finished deployments in this environment';
