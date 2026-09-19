ALTER TABLE writing_documents
  ADD COLUMN full_width INTEGER NOT NULL DEFAULT 0 CHECK(full_width IN (0,1));

PRAGMA user_version = 23;
