-- experiment table contains information about all registered experiments.
CREATE TABLE experiment (
    id INTEGER PRIMARY KEY,
    name TEXT
);

-- experiment_instance table contains information about all stored instances.
CREATE TABLE experiment_instance (
    id INTEGER PRIMARY KEY,
    -- Foreign key relationship to the experiment table.
    experiment_id INTEGER,
    -- A timestamp for when the instance was created. We can't use this to
    -- answer the get-latest query, because the timestamp's aren't at a high
    -- enough resolution.
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    -- A vault created UUID for the row. We need this as SQLite doesn't support
    -- RETURNING, so we can't get the SQLite created ID for a instance when we
    -- create it.
    uuid TEXT,
    -- The user-supplied JSON metadata for the instance. It is stored as TEXT
    -- because SQLite doesn't have a JSON column.
    meta_text TEXT,
    FOREIGN KEY (experiment_id) REFERENCES experiment (id)
);
