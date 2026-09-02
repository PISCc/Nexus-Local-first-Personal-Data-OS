CREATE TABLE IF NOT EXISTS embedding_models (
    model_id TEXT NOT NULL
        CHECK (length(trim(model_id)) > 0),
    model_version TEXT NOT NULL
        CHECK (length(trim(model_version)) > 0),
    provider_kind TEXT NOT NULL
        CHECK (length(trim(provider_kind)) > 0),
    dimensions INTEGER NOT NULL
        CHECK (dimensions > 0),
    PRIMARY KEY (model_id, model_version)
);

CREATE TABLE IF NOT EXISTS document_embeddings (
    document_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_version TEXT NOT NULL,
    dimensions INTEGER NOT NULL
        CHECK (dimensions > 0),
    source_fingerprint BLOB NOT NULL
        CHECK (length(source_fingerprint) > 0),
    vector BLOB NOT NULL
        CHECK (length(vector) = dimensions * 4),
    PRIMARY KEY (document_id, model_id, model_version),
    FOREIGN KEY (document_id) REFERENCES documents (document_id),
    FOREIGN KEY (model_id, model_version)
        REFERENCES embedding_models (model_id, model_version)
);

CREATE INDEX IF NOT EXISTS idx_document_embeddings_model
    ON document_embeddings (model_id, model_version);

CREATE TRIGGER IF NOT EXISTS documents_embeddings_after_delete
AFTER DELETE ON documents
BEGIN
    DELETE FROM document_embeddings WHERE document_id = old.document_id;
END;

CREATE TRIGGER IF NOT EXISTS documents_embeddings_after_content_update
AFTER UPDATE OF title, body ON documents
BEGIN
    DELETE FROM document_embeddings WHERE document_id = old.document_id;
END;
