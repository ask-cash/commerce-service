-- Foundation tables shared by every feature: request idempotency, the raw
-- Stripe event inbox, the outbox of events for Cash, and the audit log.
-- Feature tables (customers, subscriptions, payments, ledger) come in later
-- migrations.

-- Rows in append-only tables can be inserted but never changed or removed,
-- whichever role connects.
CREATE FUNCTION forbid_mutation() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% is append-only', TG_TABLE_NAME;
END;
$$;

-- Responses to Cash requests, keyed by the caller's Idempotency-Key.
CREATE TABLE idempotency_keys (
    tenant_id        TEXT        NOT NULL,
    key              TEXT        NOT NULL,
    request_hash     BYTEA       NOT NULL,          -- sha256 of method + path + body
    state            TEXT        NOT NULL DEFAULT 'in_progress'
                     CHECK (state IN ('in_progress', 'completed')),
    response_status  SMALLINT,
    response_body    JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at       TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '24 hours',
    PRIMARY KEY (tenant_id, key)
);
CREATE INDEX idempotency_keys_expires_at ON idempotency_keys (expires_at);

-- Every verified Stripe webhook, stored before any processing.
CREATE TABLE stripe_events (
    stripe_event_id  TEXT        PRIMARY KEY,       -- evt_..., dedupes redeliveries
    event_type       TEXT        NOT NULL,
    api_version      TEXT,
    livemode         BOOLEAN     NOT NULL,
    stripe_created   TIMESTAMPTZ NOT NULL,          -- event.created, for ordering
    payload          JSONB       NOT NULL,
    status           TEXT        NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'processed', 'ignored', 'dead')),
    attempts         INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error       TEXT,
    received_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at     TIMESTAMPTZ
);
CREATE INDEX stripe_events_due ON stripe_events (next_attempt_at) WHERE status = 'pending';

-- Events for Cash, written in the same transaction as the state change.
CREATE TABLE outbox (
    id               UUID        PRIMARY KEY,       -- UUIDv7, also the event_id Cash dedupes on
    tenant_id        TEXT        NOT NULL,
    event_type       TEXT        NOT NULL,
    payload          JSONB       NOT NULL,
    status           TEXT        NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'published', 'dead')),
    attempts         INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at     TIMESTAMPTZ
);
CREATE INDEX outbox_due ON outbox (next_attempt_at) WHERE status = 'pending';

-- Who did what, for refunds, manual replays and other privileged actions.
CREATE TABLE audit_log (
    id          UUID        PRIMARY KEY,
    tenant_id   TEXT,
    actor       TEXT        NOT NULL,               -- JWT subject of the caller
    action      TEXT        NOT NULL,
    subject     TEXT,                               -- e.g. payment or event id
    reason      TEXT,
    details     JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER audit_log_append_only
    BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION forbid_mutation();
