-- Snapshot-pipeline dogfood: error, empty-result, and float rendering.

CREATE TABLE t(x INTEGER CHECK (x > 0));

INSERT INTO t VALUES (-1); -- @snap check-violation
SELECT * FROM t; -- @snap empty
SELECT 1.5; -- 1.5
SELECT 1 + 2; -- 3
SELECT 'in snap2.sql' AS snap2; -- @snap label
