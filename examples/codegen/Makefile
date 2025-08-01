SOLITE=../../target/debug/solite


models.sql.generated.json: models.sql $(SOLITE)
	$(SOLITE) codegen $< > $@

models.py: models.sql.generated.json scripts/generate-models.py
	uv run scripts/generate-models.py $< > $@

test: models.py
	uv run test_models.py
.PHONY: test

clean:
	rm -rf models.py models.sql.generated.json
.PHONY: clean