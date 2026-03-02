# plexspaces-db

Unified database migrations for PlexSpaces. Run **once** per database (SQLite or PostgreSQL) at application startup.

## Layout

- `db/migrations/sqlite/` – SQLite migrations (001–016)
- `db/migrations/postgres/` – PostgreSQL migrations (001–016)

Same logical order in both: keyvalue, object_registrations, locks, journal (initial + actor_events + reminders), scheduling_requests, channel_messages, blob_metadata, workflow_*, tuples, barriers_and_watchers.

## Usage

Call **before** creating any store that uses the same database:

```rust
use plexspaces_db::run_migrations;

// Connection string: PostgreSQL URL or SQLite path/URL
run_migrations("postgres://user:pass@localhost/plexspaces").await?;
run_migrations("sqlite:///path/to/db.sqlite?mode=rwc").await?;
run_migrations("/path/to/db.sqlite").await?;  // normalized to sqlite URL
```

- **PostgreSQL**: URL must start with `postgres://` or `postgresql://`. Runs `db/migrations/postgres/`.
- **SQLite**: Any other string is treated as SQLite (path or `:memory:`). Runs `db/migrations/sqlite/`.

The service locator runs migrations once at startup for the configured database; individual crates (keyvalue, journaling, object-registry, etc.) then connect without running their own migrations for file/Postgres DBs. For `:memory:`, migrations are skipped at startup and each store that uses `:memory:` applies its own schema.
