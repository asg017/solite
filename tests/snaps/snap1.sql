-- Snapshot-pipeline dogfood: multi-row, text-escaping, and NULL rendering.
-- .print is kept deliberately: it's the one output-only dot command test
-- mode supports.

.print running snap1.sql

CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT);
INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob'), (3, NULL);

SELECT COUNT(*) FROM users; -- 3
SELECT * FROM users ORDER BY id; -- @snap users
SELECT name FROM users WHERE id = 3; -- NULL
SELECT 'it''s quoted text' AS v; -- @snap text-escaping
