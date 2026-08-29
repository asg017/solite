# Follow-up: make export path suffixes (`.csv.gz`, `.json.zst`, `.parquet.gz`) coherent

Status: proposals, not scheduled. Written 2026-08-29 after the parquet
exporter landed on `parquet-export` (`PLAN-parquet.md`), which made the
existing awkwardness visible: parquet is the first format for which a
compression suffix is *wrong* rather than merely optional, and the
one-off `CompressedParquet` rejection only covers one of several paths.

## What the code does today

Suffix knowledge is spread over four places that don't talk to each other:

| Where | What it knows | Consulted when |
|---|---|---|
| `exporter/mod.rs` `format_from_path()` | strips one trailing `gz`/`zst`, maps the inner extension to `ExportFormat`, rejects `parquet` + compression | only when no explicit format was given (`.export`, `query -o` without `-f`) |
| `exporter/mod.rs` `output_from_path()` | wraps the `File` in gzip/zstd based on the *final* extension only | every local-file export, regardless of how the format was chosen |
| `exporter/mod.rs` `write_output_to_bytes()` + `dot/export.rs` S3 branch | nothing — writes raw format bytes and uploads them | every `s3://`/`t3://` export |
| `replacement_scans.rs` `strip_compression_suffix()` | strips `.gz`/`.zst` so `data.csv.gz` reads as csv; relies on sqlite-xsv to actually decompress by extension | read side (`SELECT * FROM 'data.csv.gz'`) |

Plus `query.rs` `determine_format()`: `-f` wins outright; otherwise
`format_from_path`, with `Unknown` silently falling back to JSON.

Because format and compression are inferred *independently*, by
*different functions*, at *different call sites*, and the only
compatibility rule lives in one of them, the observable behavior is a grab
bag. All of these were run against the `parquet-export` build:

| Command | Result | Verdict |
|---|---|---|
| `q ... -o out.csv.gz` | gzip-wrapped CSV | correct |
| `q ... -o out.parquet.gz` | error "parquet has built-in compression" | correct |
| `q ... -f parquet -o out.parquet.gz` | **exit 0, gzip-wrapped parquet** | the rejection is bypassed because `-f` skips `format_from_path` |
| `q ... -f parquet -o out.gz` | gzip-wrapped parquet | same hole |
| `q ... -f csv -o out.json.gz` | CSV inside a `.json.gz` | `-f` silently overrides a contradicting extension |
| `q ... -f csv -o out.parquet` | CSV in a `.parquet` file | same |
| `q ... -o out.gz` | gzip-wrapped **JSON** | `Unknown` → JSON fallback, silently |
| `q ... -o out.CSV` | uncompressed JSON | extension matching is case-sensitive → Unknown → JSON |
| `q ... -o out.csv.GZ` | **uncompressed** JSON in a `.GZ` file | both functions miss it, separately |
| `q ... -o out.csv.bz2` | uncompressed JSON in a `.csv.bz2` | unknown compression suffix isn't an error, and it hides the real extension |
| `.export s3://bucket/out.csv.gz` (versitygw) | **plain CSV uploaded under a `.gz` key** | S3 path never compresses; format detection accepted the suffix |
| `.export s3://bucket/out.parquet.gz` | error | correct, but only by luck of `format_from_path` |

Two of these are real bugs (bold): gzip-wrapped parquet via `-f`, and
uncompressed `.gz` objects on S3. The rest are "silently does something
other than what the filename says" — the same smell the `.parquet.gz`
rejection was added to avoid, applied inconsistently.

## Why it's shaped this way

`format_from_path` predates `-f`, S3, and parquet. Each addition bolted on
at its own call site:

- `-f` was added as an override *of the format only*; nobody revisited
  whether the path's compression suffix should still apply (it does, by
  accident of `output_from_path` being called anyway).
- S3 reused the format writers via `write_output_to_bytes` but not
  `output_from_path`, so it inherited format detection without compression.
- Parquet's "no wrapping" rule was expressed as a special case inside
  `format_from_path` — the one function that isn't on every path.

The read side (`replacement_scans.rs`) has its own copy because it
predates the exporter's and because sqlite-xsv does the actual
decompression; it isn't wrong, just a third place to update when a
suffix is added.

## Proposals

### A. One parser, one resolver, one sink builder (recommended)

Replace the two path functions with a small pipeline in
`exporter/mod.rs` (or a new `exporter/target.rs`):

```rust
/// Everything a path's suffixes say, parsed once, case-insensitively.
pub struct PathSpec {
    pub format: Option<ExportFormat>,       // from the inner extension
    pub compression: Option<Compression>,   // Gzip | Zstd, from the outer one
    pub unknown_suffix: Option<String>,     // "bz2", "xlsx", ... for error text
}
pub fn parse_path(path: &str) -> PathSpec;    // works for local paths and s3:// keys alike

pub enum Compression { Gzip, Zstd }
impl ExportFormat {
    /// Csv/Tsv/Json/Ndjson → true; Parquet (and any future self-compressing
    /// format) → false. Value/Clipboard → false.
    pub fn supports_wrapping(&self) -> bool;
}

/// Combine what the user said (-f) with what the path says, and refuse
/// contradictions instead of picking one silently.
pub fn resolve_target(
    explicit_format: Option<ExportFormat>,
    spec: &PathSpec,
    unknown_format_default: Option<ExportFormat>,   // Some(Json) for `query`, None for `.export`
) -> Result<ResolvedTarget, TargetError>;

pub struct ResolvedTarget { pub format: ExportFormat, pub compression: Option<Compression> }

pub enum TargetError {
    UnknownFormat(String),                         // .export out.txt
    UnknownCompression { suffix: String },          // .csv.bz2
    FormatConflict { flag: ExportFormat, path: ExportFormat }, // -f csv -o x.json
    CompressionNotSupported { format: ExportFormat, compression: Compression }, // parquet + gz, via ANY path
}

/// Build the byte sink. Compression is applied here for BOTH local files and
/// object-store targets, so the S3 path stops being special.
pub fn open_sink(target: &str, compression: Option<Compression>) -> Result<Box<dyn Write + Send>, ExportError>;
```

- `open_sink` for `s3://` returns a writer that buffers (today) or the
  `WriteMultipart` adapter (after `PLAN-multipart.md`) with the gzip/zstd
  encoder layered on top — one place, both destinations. Deleting
  `write_output_to_bytes` becomes possible.
- `format_from_path` / `output_from_path` become thin wrappers or go
  away; `FormatFromPathError::CompressedParquet` is subsumed by
  `CompressionNotSupported`, which now fires for `-f parquet -o x.gz` too.
- Case-insensitive matching (`out.CSV`, `.GZ`) falls out of parsing once.
- `replacement_scans.rs` can call `parse_path` for the suffix split and
  keep its own "which vtab" decision, so a new suffix is added in one enum.

Effort: medium. Touches `exporter/mod.rs`, `dot/export.rs`, `query.rs`,
`replacement_scans.rs`, and every test that asserts on the old error
strings. Behavior changes are all from "silently wrong" to "error", so
they need a line in the docs and a snapshot re-record.

### B. Minimal patch — keep the two functions, close the holes

If A is too much for now, the same bugs can be closed without restructuring:

1. Extract `split_suffixes(path) -> (inner_ext: Option<String>, compression: Option<Compression>)`
   (case-insensitive) and use it from both `format_from_path` and
   `output_from_path` so they can never disagree.
2. Add `ExportFormat::supports_wrapping()` and a `check_compatible(format, compression)`
   called in `query.rs` **after** `-f` is applied and in `dot/export.rs`;
   this is what stops `-f parquet -o x.parquet.gz`.
3. In the S3 branch of `dot/export.rs`, wrap the `Vec<u8>` in the same
   gzip/zstd encoder before uploading (a `compress_bytes(bytes, compression)`
   helper), or better, route it through a `Write`-based sink so it's the
   same code as local files.
4. Treat an unrecognized *outer* suffix (`.bz2`, `.xz`) as
   `UnknownCompression` rather than as the format extension.

Effort: small. Leaves the "three call sites" shape in place, so the next
format/destination can re-open the holes.

### C. Make compression explicit and demote suffix inference to a default

Orthogonal to A/B: add `--compress <gz|zst|none>` to `solite query` and an
optional `--compress` on `.export`. The path suffix becomes the *default*
for that flag rather than the only way to ask. Then:

- `-f parquet --compress gz` is a clear, early error ("parquet compresses
  internally") with no path parsing involved.
- `-o out.gz` with `-f csv` is unambiguous: format csv, compression gz,
  and the filename is the user's business.
- Scripts that pipe to stdout (`-f csv --compress zst > out.csv.zst`) get
  compression for the first time — today stdout is never compressed
  because only `output_from_path` knows how.

This doesn't remove the need for A/B (the suffix path still has to be
parsed consistently), but it gives users a way around inference and makes
the stdout gap go away.

## Policy decisions to settle before implementing

These are product calls, not code cleanups; whichever proposal is picked
needs an answer to each:

1. **`-f` vs a contradicting extension** (`-f csv -o x.json`): error
   (recommended — it's almost always a typo), warn on stderr, or trust
   `-f` silently (today).
2. **Unknown extension in `query -o`** (`-o out.txt`): keep the silent
   JSON default (today; the help text says "json otherwise"), or require
   `-f` when the path doesn't say. Recommended: keep the JSON default but
   make it case-insensitive and make unknown *compression* suffixes an
   error (`.csv.bz2` should not quietly become uncompressed JSON).
3. **Case sensitivity**: `out.CSV` and `out.csv.GZ` should work
   (recommended); today they silently degrade.
4. **Compression on stdout / `-f` with no `-o`**: only via an explicit
   flag (proposal C) — never by guessing.
5. **Self-compressing formats**: parquet is the first; a future
   `.sqlite`/`.db` export or `.xlsx` would be the same class. Model it as
   `supports_wrapping()` on the format rather than a per-format special
   case.

## Suggested sequencing

1. B.2 + B.3 now (two real bugs: gzip-wrapped parquet via `-f`, uncompressed
   `.gz` on S3) — small, testable with the existing `s3_gateway` fixture.
2. A when `PLAN-multipart.md` is picked up — the streaming sink is the
   natural moment to introduce `open_sink`, since the S3 writer becomes a
   `Write` anyway and compression layering is one `Encoder::new` away.
3. C if/when someone asks for compressed stdout or hits the `-f`/suffix
   ambiguity in practice.

## Tests worth adding regardless

- `-f parquet -o x.parquet.gz` and `-f parquet -o x.gz` → error, no file.
- `.export s3://bucket/x.csv.gz` (versitygw) → object is gzip and
  decompresses to the CSV.
- `-o out.CSV`, `-o out.csv.GZ` → csv, gzip.
- `-o out.csv.bz2` → error naming `bz2`.
- `-f csv -o out.json` → whatever decision 1 lands on, asserted explicitly.
