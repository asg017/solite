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
