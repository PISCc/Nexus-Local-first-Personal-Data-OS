CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    document_id UNINDEXED,
    title,
    body,
    content = 'documents',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 1'
);

CREATE TRIGGER IF NOT EXISTS documents_fts_after_insert
AFTER INSERT ON documents
BEGIN
    INSERT INTO documents_fts (rowid, document_id, title, body)
    VALUES (new.rowid, new.document_id, new.title, new.body);
END;

CREATE TRIGGER IF NOT EXISTS documents_fts_after_delete
AFTER DELETE ON documents
BEGIN
    INSERT INTO documents_fts (documents_fts, rowid, document_id, title, body)
    VALUES ('delete', old.rowid, old.document_id, old.title, old.body);
END;

CREATE TRIGGER IF NOT EXISTS documents_fts_after_update
AFTER UPDATE ON documents
BEGIN
    INSERT INTO documents_fts (documents_fts, rowid, document_id, title, body)
    VALUES ('delete', old.rowid, old.document_id, old.title, old.body);
    INSERT INTO documents_fts (rowid, document_id, title, body)
    VALUES (new.rowid, new.document_id, new.title, new.body);
END;

INSERT INTO documents_fts (documents_fts) VALUES ('rebuild');
