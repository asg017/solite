def test_list_rules(solite_cli):
    result = solite_cli(["lint", "--list-rules"])
    assert result.success
    for rule_id in ["double-quoted-string", "empty-blob-literal", "missing-as"]:
        assert rule_id in result.stdout
    assert "(fixable)" in result.stdout
