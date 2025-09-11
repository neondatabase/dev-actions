CREATE TABLE deployments (
    -- Primary key with auto-incrementing BIGSERIAL for ordering
    id BIGSERIAL PRIMARY KEY,
    
    -- Deployment metadata fields (filled at insertion)
    region VARCHAR(100) NOT NULL,
    environment VARCHAR(50) NOT NULL,
    component VARCHAR(200) NOT NULL,
    version VARCHAR(100) NOT NULL,
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

-- Trigger to automatically update updated_at on row updates
CREATE TRIGGER update_deployments_updated_at 
    BEFORE UPDATE ON deployments 
    FOR EACH ROW 
    EXECUTE FUNCTION update_updated_at_column();

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


