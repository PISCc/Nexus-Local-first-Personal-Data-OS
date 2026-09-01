CREATE TABLE IF NOT EXISTS file_metadata (
    path_key BLOB PRIMARY KEY NOT NULL,
    path_display TEXT NOT NULL,
    file_name TEXT NOT NULL,
    extension TEXT,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    modified_at INTEGER,
    created_at INTEGER,
    accessed_at INTEGER,
    file_type TEXT
);

CREATE INDEX IF NOT EXISTS idx_file_metadata_extension
    ON file_metadata (extension);

CREATE INDEX IF NOT EXISTS idx_file_metadata_modified_at
    ON file_metadata (modified_at);
