-- Make more fields immutable by updating the validation trigger
-- Adds created_at and concurrency_key to the list of immutable fields.
-- These fields define the identity and scheduling of a deployment and should not change after creation.

CREATE OR REPLACE FUNCTION validate_deployment_state_transition()
RETURNS TRIGGER AS $$
BEGIN
    -- Prevent changes to immutable fields
    -- Immutable fields: id, region, environment, component, version, url, note, concurrency_key, created_at
    IF (OLD.id IS DISTINCT FROM NEW.id
        OR OLD.region IS DISTINCT FROM NEW.region
        OR OLD.environment IS DISTINCT FROM NEW.environment
        OR OLD.component IS DISTINCT FROM NEW.component
        OR OLD.version IS DISTINCT FROM NEW.version
        OR OLD.url IS DISTINCT FROM NEW.url
        OR OLD.note IS DISTINCT FROM NEW.note
        OR OLD.concurrency_key IS DISTINCT FROM NEW.concurrency_key
        OR OLD.created_at IS DISTINCT FROM NEW.created_at) THEN
        RAISE EXCEPTION 'Cannot modify immutable fields for deployment %', OLD.id;
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
