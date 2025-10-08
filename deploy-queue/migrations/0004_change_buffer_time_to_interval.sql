-- Change buffer_time from integer minutes to interval type
-- This makes the type system more accurate and simplifies interval arithmetic
-- Update the buffer_time column type from INTEGER to INTERVAL
-- Convert existing minute values to proper intervals
ALTER TABLE
    environments
ALTER COLUMN
    buffer_time TYPE INTERVAL USING (buffer_time || ' minutes')::INTERVAL;

-- Update the column comment to reflect the new type
COMMENT ON COLUMN environments.buffer_time IS 'Buffer time interval for finished deployments in this environment';
