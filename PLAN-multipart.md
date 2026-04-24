# Plan: Streaming multipart uploads for `.export` to object storage

## Context

The current MVP buffers the entire export in memory (`write_output_to_bytes`) then uploads in a single `put()`. This won't work for large exports (100s of MB+). The `object_store` crate provides `WriteMultipart` which streams 5MB chunks in parallel — and its `write()` method is **synchronous**, which fits perfectly into the existing `W: Write` pipeline.

## Key insight

`WriteMultipart::write(&mut self, buf: &[u8])` is sync. It internally buffers data and spawns tokio tasks for 5MB part uploads automatically. Only `finish()` is async (flushes the last chunk and completes the upload).

The existing export writers (`write_csv`, `write_json`, etc.) are all generic over `W: Write`. So we just need a thin `std::io::Write` adapter around `WriteMultipart`, and the entire export streams directly to S3 with zero buffering changes.

## Implementation

### 1. Replace `upload()` with `streaming_upload()` in `crates/solite-core/src/object_store.rs`

```rust
/// Adapter: std::io::Write → WriteMultipart::write() (sync)
struct MultipartWriter {
    inner: WriteMultipart,
}

impl std::io::Write for MultipartWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf);  // sync — buffers + auto-uploads 5MB parts
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())  // real flush happens in finish()
    }
}

/// Export to object store using streaming multipart upload.
pub fn streaming_upload(
    url: &str,
    write_fn: impl FnOnce(&mut dyn Write) -> Result<(), ExportError>,
) -> Result<(), ExportError>
```

Flow inside `streaming_upload`:
1. Parse URL, build `AmazonS3`
2. Create tokio runtime
3. `block_on`: `put_multipart()` → `MultipartUpload`
4. Wrap in `WriteMultipart` → `MultipartWriter`
5. Call `write_fn(&mut writer)` — sync, rows stream directly to S3
6. `block_on`: `writer.inner.finish()` to complete
7. On error: `block_on`: `abort()` to clean up partial parts

### 2. Update `ExportCommand::execute()` in `crates/solite-core/src/dot/export.rs`

```rust
if object_store::is_object_store_url(&target_str) {
    object_store::streaming_upload(&target_str, |writer| {
        write_output_to_writer(&mut self.statement, writer, format)
    })?;
}
```

### 3. Add `write_output_to_writer` in `crates/solite-core/src/exporter.rs`

Takes `&mut dyn Write` instead of `Box<dyn Write>`. Remove `write_output_to_bytes`.

## Files to modify

| File | Change |
|------|--------|
| `crates/solite-core/src/object_store.rs` | Replace `upload()` with `streaming_upload()` + `MultipartWriter` |
| `crates/solite-core/src/dot/export.rs` | Use closure-based `streaming_upload` |
| `crates/solite-core/src/exporter.rs` | Add `write_output_to_writer`, remove `write_output_to_bytes` |

---

## Conditional puts / overwrite protection

`object_store` supports `PutMode` via `put_opts()`:

```rust
pub enum PutMode {
    Overwrite,                  // default — always write
    Create,                     // fail with AlreadyExists if key exists
    Update(UpdateVersion),      // CAS — fail with Precondition if etag/version doesn't match
}
```

### Brainstorm: how to expose this in `.export`

**Option A: Flags on `.export`**

```sql
.export --no-clobber s3://bucket/results.csv
SELECT * FROM users

.export --if-match "etag-value" s3://bucket/results.csv
SELECT * FROM users
```

- `--no-clobber` / `--create` → `PutMode::Create`. Fails if object already exists. Simple and safe for scheduled/cron exports that shouldn't overwrite.
- `--if-match <etag>` → `PutMode::Update(UpdateVersion { e_tag })`. CAS semantics — only overwrite if the current version matches. Useful for read-modify-write patterns.
- Default (no flag) → `PutMode::Overwrite` (current behavior).

Pros: explicit, familiar (mimics `cp --no-clobber`), no new commands.
Cons: flags on dot commands are unusual in this codebase (only `.bench --name` does it). Etag values are opaque strings that users would need to obtain separately.

**Option B: Separate `.upload` command with options**

```sql
.upload s3://bucket/results.csv --create
SELECT * FROM users
```

New dot command dedicated to object store exports. Could carry richer options without cluttering `.export`.

Pros: clean separation of local vs remote concerns.
Cons: another dot command to maintain, feature fragmentation (now two ways to export).

**Option C: Automatic `--no-clobber` via path convention**

```sql
-- Timestamp in the key means no collisions
.export s3://bucket/exports/:date/results.csv
SELECT * FROM users
```

Lean on the existing parameter substitution (`:date`, `:timestamp`) to generate unique keys. No conditional put needed — each export goes to a new path.

Pros: zero new API surface, idiomatic for data pipelines.
Cons: doesn't solve the case where you genuinely want "don't overwrite this exact key". Bucket accumulates files.

**Option D: `.param`-based configuration**

```sql
.param set export_mode create
.export s3://bucket/results.csv
SELECT * FROM users
```

Use the existing parameter system to set export behavior. The object store module reads `export_mode` from runtime parameters.

Pros: no new syntax, uses existing system.
Cons: action-at-a-distance, easy to forget it's set, surprising behavior.

### Recommendation

**Option A (flags)** for the MVP, with just `--no-clobber`:

```sql
.export --no-clobber s3://bucket/results.csv
SELECT * FROM users
```

- Only applies to object store URLs (local files already have OS-level overwrite behavior)
- Single flag, no etag complexity yet
- Parse it in `ExportCommand::new()` by splitting args before the URL
- `PutMode::Create` maps directly to S3's `If-None-Match: *` header
- Tigris supports this natively

`--if-match` can be added later if CAS is needed. The multipart path (`put_multipart`) doesn't support `PutOptions` directly, but for `--no-clobber` we can do a `head()` check before starting the upload (not perfectly atomic, but good enough for most cases; for small exports, `put_opts` with `PutMode::Create` is atomic).
