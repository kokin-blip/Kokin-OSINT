CREATE TABLE face_run (
 id TEXT PRIMARY KEY NOT NULL REFERENCES transform_run(id),
 artifact_id TEXT NOT NULL REFERENCES artifact(id), model_identity TEXT NOT NULL,
 page INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER,
 sampled_frames INTEGER NOT NULL, complete INTEGER NOT NULL CHECK(complete IN(0,1)),
 limitations_json TEXT NOT NULL CHECK(json_valid(limitations_json)), created_utc TEXT NOT NULL
) STRICT;
CREATE INDEX face_run_artifact ON face_run(artifact_id);
CREATE TABLE face_detection (
 id TEXT PRIMARY KEY NOT NULL, run_id TEXT NOT NULL REFERENCES face_run(id),
 artifact_id TEXT NOT NULL REFERENCES artifact(id), model_identity TEXT NOT NULL,
 locator_json TEXT NOT NULL CHECK(json_valid(locator_json)), detector_response REAL NOT NULL,
 vector BLOB CHECK(vector IS NULL OR length(vector)=512), crop_png BLOB,
 stale INTEGER NOT NULL DEFAULT 0 CHECK(stale IN(0,1)), created_utc TEXT NOT NULL
) STRICT;
CREATE INDEX face_detection_run ON face_detection(run_id);
CREATE INDEX face_detection_artifact ON face_detection(artifact_id,stale);
CREATE TABLE face_group (
 id TEXT PRIMARY KEY NOT NULL, model_identity TEXT NOT NULL, created_utc TEXT NOT NULL
) STRICT;
CREATE TABLE face_group_member (
 group_id TEXT NOT NULL REFERENCES face_group(id), face_id TEXT NOT NULL REFERENCES face_detection(id),
 PRIMARY KEY(group_id,face_id)
) STRICT;
CREATE TABLE face_review (
 id INTEGER PRIMARY KEY AUTOINCREMENT, face_id TEXT NOT NULL REFERENCES face_detection(id),
 entity_id TEXT REFERENCES entity(id), group_id TEXT REFERENCES face_group(id),
 decision TEXT NOT NULL CHECK(decision IN('accepted','rejected','unlinked','excluded')),
 rationale TEXT NOT NULL CHECK(length(trim(rationale))>0),
 actor TEXT NOT NULL CHECK(actor='user:local'), created_utc TEXT NOT NULL
) STRICT;
CREATE INDEX face_review_face ON face_review(face_id,id);
CREATE TRIGGER face_review_no_update BEFORE UPDATE ON face_review BEGIN SELECT RAISE(ABORT,'face reviews are append-only'); END;
CREATE TRIGGER face_review_no_delete BEFORE DELETE ON face_review BEGIN SELECT RAISE(ABORT,'face reviews are append-only'); END;
CREATE TABLE face_preferences (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), auto_analyze INTEGER NOT NULL CHECK(auto_analyze IN(0,1))
) STRICT;
INSERT INTO face_preferences VALUES(1,0);
CREATE TRIGGER face_source_shredded AFTER UPDATE OF wrapped_key ON blob WHEN NEW.wrapped_key IS NULL BEGIN
 UPDATE face_detection SET vector=NULL,crop_png=NULL,stale=1
 WHERE artifact_id IN(SELECT id FROM artifact WHERE content_hash=NEW.content_hash);
END;
