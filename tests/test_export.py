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
