CREATE TABLE research_run (
 id TEXT PRIMARY KEY NOT NULL,
 plan_json TEXT NOT NULL CHECK(json_valid(plan_json)),
 status TEXT NOT NULL CHECK(status IN('running','completed','partial','cancelled','interrupted')),
 created_utc TEXT NOT NULL, finished_utc TEXT,
 expected INTEGER NOT NULL CHECK(expected>=0), completed INTEGER NOT NULL DEFAULT 0 CHECK(completed>=0)
) STRICT;
CREATE TABLE research_result (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,run_id TEXT NOT NULL REFERENCES research_run(id),
 cache_key TEXT NOT NULL, outcome TEXT NOT NULL,
 artifact_id TEXT REFERENCES artifact(id),
 payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
 created_epoch INTEGER NOT NULL
) STRICT;
CREATE INDEX research_result_run ON research_result(run_id,sequence);
CREATE INDEX research_cache ON research_result(cache_key,created_epoch);
CREATE TRIGGER research_result_no_update BEFORE UPDATE ON research_result BEGIN SELECT RAISE(ABORT,'research results are append-only'); END;
CREATE TRIGGER research_result_no_delete BEFORE DELETE ON research_result BEGIN SELECT RAISE(ABORT,'research results are append-only'); END;
CREATE TABLE research_document (
 id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),
 kind TEXT NOT NULL CHECK(kind IN('collection','tool_preferences','saved_search','monitor','suppression','dossier','annotations','crawl','template')),
 name TEXT NOT NULL,payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
 archived INTEGER NOT NULL CHECK(archived IN(0,1)),updated_utc TEXT NOT NULL,
 PRIMARY KEY(id,revision)
) STRICT;
CREATE INDEX research_document_kind ON research_document(kind,id,revision);
CREATE TRIGGER research_document_no_update BEFORE UPDATE ON research_document BEGIN SELECT RAISE(ABORT,'research documents are revisioned'); END;
CREATE TRIGGER research_document_no_delete BEFORE DELETE ON research_document BEGIN SELECT RAISE(ABORT,'research documents are revisioned'); END;
