-- Derived query artifacts are L1, never fresh L0 captures or identity findings.
CREATE TABLE image_query (
 artifact_id TEXT PRIMARY KEY NOT NULL REFERENCES artifact(id),
 source_artifact_id TEXT NOT NULL REFERENCES artifact(id),
 run_id TEXT NOT NULL REFERENCES transform_run(id),
 page INTEGER NOT NULL CHECK(page>=0),
 recipe_json TEXT NOT NULL CHECK(json_valid(recipe_json)),
 engine TEXT NOT NULL, created_utc TEXT NOT NULL,
 CHECK(artifact_id<>source_artifact_id)
) STRICT;
CREATE INDEX image_query_source ON image_query(source_artifact_id);
CREATE TRIGGER image_query_no_update BEFORE UPDATE ON image_query BEGIN SELECT RAISE(ABORT,'image preparation lineage is append-only'); END;
CREATE TRIGGER image_query_no_delete BEFORE DELETE ON image_query BEGIN SELECT RAISE(ABORT,'image preparation lineage is append-only'); END;
-- A source shred must also revoke nested query crops and their native previews.
CREATE TRIGGER image_query_source_shredded AFTER UPDATE OF wrapped_key ON blob WHEN NEW.wrapped_key IS NULL AND OLD.wrapped_key IS NOT NULL BEGIN
 UPDATE blob SET wrapped_key=NULL,wrapped_nonce=NULL,shredded_utc=NEW.shredded_utc
 WHERE content_hash IN (
  WITH RECURSIVE descendants(id) AS (
   SELECT id FROM artifact WHERE content_hash=NEW.content_hash
   UNION SELECT q.artifact_id FROM image_query q JOIN descendants d ON q.source_artifact_id=d.id
  )
  SELECT a.content_hash FROM artifact a JOIN descendants d ON a.id=d.id
  UNION SELECT m.thumbnail_hash FROM media_derivative m JOIN descendants d ON m.artifact_id=d.id
  UNION SELECT m.snapshot_hash FROM media_derivative m JOIN descendants d ON m.artifact_id=d.id
 ) AND wrapped_key IS NOT NULL;
END;
