CREATE TABLE IF NOT EXISTS documents (
    document_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(trim(document_id)) > 0),
    source_kind TEXT NOT NULL
        CHECK (source_kind = 'local_file'),
    source_path_key BLOB NOT NULL
        CHECK (length(source_path_key) > 0),
    source_path_display TEXT NOT NULL
        CHECK (length(source_path_display) > 0),
    title TEXT NOT NULL
        CHECK (length(trim(title)) > 0),
    body TEXT NOT NULL,
    line_start INTEGER,
    line_end INTEGER,
    CHECK (
        (line_start IS NULL AND line_end IS NULL)
        OR (
            line_start IS NOT NULL
            AND line_end IS NOT NULL
            AND line_start > 0
            AND line_end >= line_start
        )
    )
);

CREATE INDEX IF NOT EXISTS idx_documents_source_path
    ON documents (source_path_key);
