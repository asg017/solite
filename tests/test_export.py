import gzip
import json
import subprocess

import pyarrow.parquet as pq

from conftest import CLI_PATH


def test_export_s3_without_creds_is_not_a_file_error(solite_cli, tmp_path):
    """Regression test for the object_store feature-forwarding bug.

    Without `solite-cli` forwarding solite-core's `object_store` feature,
    `.export s3://...` was compiled out and silently fell through to
    `File::create("s3://...")`, failing with an ENOENT-style "No such file
    or directory" error. Pointing at an endpoint nothing listens on (with
    dummy creds) proves the real S3 code path is compiled in: it must fail
    for a network/connection reason, never a "file not found" reason.
    """
    script = tmp_path / "s3.sql"
    script.write_text(".export s3://bucket/out.csv\nselect 1 as a;\n")

    result = solite_cli(
        ["run", str(script)],
        env={
            "AWS_ENDPOINT_URL_S3": "http://127.0.0.1:1",
            "AWS_ACCESS_KEY_ID": "dummy",
            "AWS_SECRET_ACCESS_KEY": "dummy",
            "AWS_REGION": "us-east-1",
        },
    )

    assert not result.success
    assert "No such file or directory" not in result.stderr


def test_export_s3_csv_roundtrip(solite_cli, s3_gateway, tmp_path):
    script = tmp_path / "s3.sql"
    script.write_text(".export s3://bucket/dir/out.csv\nselect 1 as a, 'x' as b;\n")

    result = solite_cli(["run", str(script)], env=s3_gateway.env)

    assert result.success, result.stderr
    uploaded = s3_gateway.root / "bucket" / "dir" / "out.csv"
    assert uploaded.read_text() == "a,b\n1,x\n"


def test_export_t3_alias_csv_roundtrip(solite_cli, s3_gateway, tmp_path):
    script = tmp_path / "t3.sql"
    script.write_text(".export t3://bucket/out2.csv\nselect 1 as a, 'x' as b;\n")

    result = solite_cli(["run", str(script)], env=s3_gateway.env)

    assert result.success, result.stderr
    uploaded = s3_gateway.root / "bucket" / "out2.csv"
    assert uploaded.read_text() == "a,b\n1,x\n"


def test_query_parquet_output_file(solite_cli, snapshot, tmp_path):
    sql = "select * from json_tree('[1,2,3,4]')"
    result = solite_cli(["q", sql, "-o", "a.parquet"], cwd=tmp_path)
    assert result.success, result.stderr

    table = pq.read_table(tmp_path / "a.parquet")
    assert str(table.schema) == snapshot(name="a.parquet schema")
    assert table.to_pylist() == snapshot(name="a.parquet rows")


def test_query_parquet_stdout(tmp_path):
    out_file = tmp_path / "out.parquet"
    with open(out_file, "wb") as f:
        result = subprocess.run(
            [str(CLI_PATH), "q", "select 1 as a, 'x' as b", "-f", "parquet"],
            stdout=f,
        )
    assert result.returncode == 0

    table = pq.read_table(out_file)
    assert table.to_pylist() == [{"a": 1, "b": "x"}]


def test_query_parquet_types(solite_cli, snapshot, tmp_path):
    script = tmp_path / "types.sql"
    script.write_text(
        """
        create table t(a integer, b real, c text, d blob, e boolean, j text);
        insert into t values (1, 1.5, 'hello', x'deadbeef', 1, json('{"k":1}'));
        insert into t values (null, null, null, null, null, null);
        .export out.parquet
        select * from t;
        """
    )

    result = solite_cli(["run", str(script)], cwd=tmp_path)
    assert result.success, result.stderr

    table = pq.read_table(tmp_path / "out.parquet")
    assert str(table.schema) == snapshot(name="types.parquet schema")
    assert table.to_pylist() == snapshot(name="types.parquet rows")


def test_query_parquet_compressed_extension_rejected(solite_cli, tmp_path):
    sql = "select 1 as a"
    result = solite_cli(["q", sql, "-o", "a.parquet.gz"], cwd=tmp_path)

    assert not result.success
    assert "built-in compression" in result.stderr
    assert not (tmp_path / "a.parquet.gz").exists()


def test_export_parquet_type_mismatch(solite_cli, tmp_path):
    script = tmp_path / "mismatch.sql"
    script.write_text(
        """
        create table t(a integer);
        insert into t values (1);
        insert into t values (2);
        insert into t values ('oops');
        .export out.parquet
        select * from t;
        """
    )

    result = solite_cli(["run", str(script)], cwd=tmp_path)

    assert not result.success
    assert "row 3" in result.stderr
    assert "INT64" in result.stderr
    assert "declared type 'INTEGER'" in result.stderr
    # The file is created eagerly before rows are written (same as csv
    # today via `output_from_path`), so a partial file is left behind on
    # error rather than being cleaned up.
    assert (tmp_path / "out.parquet").exists()


def test_export_parquet_s3(solite_cli, s3_gateway, tmp_path):
    script = tmp_path / "s3.sql"
    script.write_text(".export s3://bucket/dir/out.parquet\nselect 1 as a;\n")

    result = solite_cli(["run", str(script)], env=s3_gateway.env)

    assert result.success, result.stderr
    uploaded = s3_gateway.root / "bucket" / "dir" / "out.parquet"
    table = pq.read_table(uploaded)
    assert table.to_pylist() == [{"a": 1}]


def test_export_geojson_collection(solite_cli, tmp_path):
    script = tmp_path / "geo.sql"
    script.write_text(
        """
        create table t(id integer, name text, geometry text);
        insert into t values (1, 'a', json('{"type":"Point","coordinates":[1,2]}'));
        insert into t values (2, 'b', '{"type":"LineString","coordinates":[[0,0],[1,1]]}');
        .export out.geojson
        select * from t;
        """
    )

    result = solite_cli(["run", str(script)], cwd=tmp_path)
    assert result.success, result.stderr

    data = json.loads((tmp_path / "out.geojson").read_text())
    assert data["type"] == "FeatureCollection"
    assert len(data["features"]) == 2
    assert data["features"][0]["properties"] == {"id": 1, "name": "a"}
    assert data["features"][1]["geometry"]["type"] == "LineString"


def test_export_geojsonl_gz(solite_cli, tmp_path):
    script = tmp_path / "geo.sql"
    script.write_text(
        """
        create table t(id integer, geometry text);
        insert into t values (1, json('{"type":"Point","coordinates":[1,2]}'));
        insert into t values (2, json('{"type":"Point","coordinates":[3,4]}'));
        .export out.geojsonl.gz
        select * from t;
        """
    )

    result = solite_cli(["run", str(script)], cwd=tmp_path)
    assert result.success, result.stderr

    lines = gzip.open(tmp_path / "out.geojsonl.gz").read().decode().splitlines()
    assert len(lines) == 2
    for line in lines:
        assert json.loads(line)["type"] == "Feature"


def test_export_geojsons(solite_cli, tmp_path):
    script = tmp_path / "geo.sql"
    script.write_text(
        """
        create table t(id integer, geometry text);
        insert into t values (1, json('{"type":"Point","coordinates":[1,2]}'));
        insert into t values (2, json('{"type":"Point","coordinates":[3,4]}'));
        .export out.geojsons
        select * from t;
        """
    )

    result = solite_cli(["run", str(script)], cwd=tmp_path)
    assert result.success, result.stderr

    # `str.splitlines()` treats the ASCII record separator (0x1e) as its
    # own line boundary, so split on "\n" directly to keep each record
    # (which starts with 0x1e) intact.
    text = (tmp_path / "out.geojsons").read_text()
    lines = [line for line in text.split("\n") if line]
    assert len(lines) == 2
    for line in lines:
        assert line.startswith("\x1e")
        json.loads(line[1:])


def test_query_geojson_stdout(solite_cli, tmp_path):
    sql = (
        'select 1 as id, json(\'{"type":"Point","coordinates":[1,2]}\') as geometry'
    )
    result = solite_cli(["q", sql, "-f", "geojson"])
    assert result.success, result.stderr

    data = json.loads(result.stdout)
    assert data["type"] == "FeatureCollection"
    assert len(data["features"]) == 1

    result = solite_cli(["q", sql, "-o", "out.geojsonl"], cwd=tmp_path)
    assert result.success, result.stderr
    assert (tmp_path / "out.geojsonl").exists()


def test_export_geojson_missing_geometry_column(solite_cli, tmp_path):
    script = tmp_path / "geo.sql"
    script.write_text(".export out.geojson\nselect 1 as id;\n")

    result = solite_cli(["run", str(script)], cwd=tmp_path)

    assert not result.success
    assert "no geometry column" in result.stderr
    assert "id" in result.stderr


def test_export_geojson_wkt_rejected(solite_cli, tmp_path):
    script = tmp_path / "geo.sql"
    script.write_text(".export out.geojson\nselect 'POINT(1 2)' as geometry;\n")

    result = solite_cli(["run", str(script)], cwd=tmp_path)

    assert not result.success
    assert "row 1" in result.stderr
    assert "tg_to_geojson" in result.stderr


def test_export_geojson_s3(solite_cli, s3_gateway, tmp_path):
    script = tmp_path / "s3.sql"
    script.write_text(
        ".export s3://bucket/dir/out.geojson\n"
        "select json('{\"type\":\"Point\",\"coordinates\":[1,2]}') as geometry;\n"
    )

    result = solite_cli(["run", str(script)], env=s3_gateway.env)

    assert result.success, result.stderr
    uploaded = s3_gateway.root / "bucket" / "dir" / "out.geojson"
    data = json.loads(uploaded.read_text())
    assert data["type"] == "FeatureCollection"
    assert len(data["features"]) == 1
