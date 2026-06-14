use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn generate_builtins() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let mut stmt = conn
        .prepare("SELECT DISTINCT name FROM pragma_function_list WHERE builtin order by 1")
        .unwrap();
    let functions: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest_path = out_dir.join("builtins.rs");

    let mut file = std::fs::File::create(dest_path).unwrap();

    writeln!(
        file,
        "pub static BUILTIN_FUNCTIONS: &[&str] = &{:?};",
        functions
    )
    .unwrap();
}

fn build_amalgamation_if_needed(sqlite_dir: &Path) -> PathBuf {
    let sqlite3_c = sqlite_dir.join("sqlite3.c");

    // sqlite3.c is generated and untracked, so checking out a different
    // submodule commit leaves the previous version's amalgamation on disk.
    // Stamp the generated sources with manifest.uuid (the exact source
    // checkout id) and regenerate whenever it doesn't match.
    let stamp_path = sqlite_dir.join(".solite-amalgamation-stamp");
    let manifest_uuid = std::fs::read_to_string(sqlite_dir.join("manifest.uuid"))
        .expect("vendor/sqlite/manifest.uuid missing — is the submodule checked out? (git submodule update --init)");
    let stamp_matches = std::fs::read_to_string(&stamp_path)
        .map(|s| s == manifest_uuid)
        .unwrap_or(false);

    if sqlite3_c.exists() && !stamp_matches {
        // Stale amalgamation from a previous checkout: clean the generated
        // sources. make clean can exit nonzero yet still do its job, so
        // verify by deleting the key outputs ourselves if they survived.
        let _ = Command::new("make")
            .arg("clean")
            .current_dir(sqlite_dir)
            .status();
        for f in ["sqlite3.c", "sqlite3.h", "shell.c"] {
            let _ = std::fs::remove_file(sqlite_dir.join(f));
        }
    }

    if !sqlite3_c.exists() {
        // On Windows, ./configure is a shell script so we invoke it via sh.
        // make/sh are available via Git for Windows / MSYS2.
        let configure_cmd = if cfg!(target_os = "windows") { "sh" } else { "./configure" };
        let configure_args: &[&str] = if cfg!(target_os = "windows") { &["./configure"] } else { &[] };
        let status = Command::new(configure_cmd)
            .args(configure_args)
            .current_dir(sqlite_dir)
            .status()
            .expect("Failed to run ./configure in sqlite submodule. On Windows, ensure Git for Windows (sh) and make are installed.");
        if !status.success() {
            panic!("./configure failed in sqlite submodule");
        }

        // SQLITE_ENABLE_UPDATE_DELETE_LIMIT changes the SQL grammar, so it
        // must be present when Lemon generates parse.c — i.e. during
        // amalgamation creation, not just at compile time.
        let make_opts = "OPTS=-DSQLITE_ENABLE_UPDATE_DELETE_LIMIT";

        // sqlite3.h must be built before sqlite3.c due to Makefile dependency ordering
        let status = Command::new("make")
            .args(["sqlite3.h", make_opts])
            .current_dir(sqlite_dir)
            .status()
            .expect("Failed to run make sqlite3.h in sqlite submodule");
        if !status.success() {
            panic!("make sqlite3.h failed in sqlite submodule");
        }

        let status = Command::new("make")
            .args(["sqlite3.c", make_opts])
            .current_dir(sqlite_dir)
            .status()
            .expect("Failed to run make sqlite3.c in sqlite submodule");
        if !status.success() {
            panic!("make sqlite3.c failed in sqlite submodule");
        }

        std::fs::write(&stamp_path, &manifest_uuid)
            .expect("failed to write amalgamation stamp");
    }
    sqlite_dir.to_path_buf()
}

fn build_sqlite_extension(
    name: &str,
    sqlite_dir: &Path,
    amalgamation_dir: &Path,
    c_opt_level: u32,
    extra_includes: &[PathBuf],
) {
    let c_file = sqlite_dir.join(format!("ext/misc/{name}.c"));
    let mut build = cc::Build::new();
    build
        .file(&c_file)
        .opt_level(c_opt_level)
        .warnings(false)
        .include(amalgamation_dir);
    for inc in extra_includes {
        build.include(inc);
    }
    if cfg!(feature = "static") {
        build.define("SQLITE_CORE", None);
    }
    if name == "fileio" && cfg!(target_os = "windows") {
        // windirent.h lives in ext/misc/ alongside fileio.c and is header-only,
        // so just make sure ext/misc is on the include path.
        build.include(sqlite_dir.join("ext/misc"));
    }
    build.compile(name);
}

fn main() {
    generate_builtins();

    // Use -O0 for dev builds to speed up C compilation (~15s savings)
    let c_opt_level: u32 = if env::var("PROFILE").unwrap_or_default() == "release" {
        3
    } else {
        0
    };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let sqlite_dir = manifest_dir.join("../../vendor/sqlite");

    println!("cargo:rerun-if-env-changed=SOLITE_AMALGAMMATION_DIR");
    // Re-run when the submodule checkout moves so the stamp check in
    // build_amalgamation_if_needed() gets a chance to regenerate.
    println!(
        "cargo:rerun-if-changed={}",
        sqlite_dir.join("manifest.uuid").display()
    );
    let amalgamation_dir = env::var("SOLITE_AMALGAMMATION_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| build_amalgamation_if_needed(&sqlite_dir));

    println!(
        "cargo:rerun-if-changed={}",
        amalgamation_dir.join("sqlite3.c").display()
    );

    let mut sqlite_build = cc::Build::new();
    sqlite_build
        .file(amalgamation_dir.join("sqlite3.c"))
        .include(&amalgamation_dir)
        .opt_level(c_opt_level)
        .define("SQLITE_ENABLE_RTREE", None)
        .define("SQLITE_SOUNDEX", None)
        .define("SQLITE_ENABLE_GEOPOLY", None)
        .define("SQLITE_ENABLE_MATH_FUNCTIONS", None)
        .define("SQLITE_ENABLE_FTS5", None)
        .define("SQLITE_ENABLE_DBSTAT_VTAB", None)
        .define("SQLITE_ENABLE_STMTVTAB", None)
        .define("SQLITE_ENABLE_BYTECODE_VTAB", None)
        .define("SQLITE_ENABLE_EXPLAIN_COMMENTS", None)
        .define("SQLITE_ENABLE_STMT_SCANSTATUS", None)
        .define("SQLITE_ENABLE_COLUMN_METADATA", None)
        .define("SQLITE_ENABLE_UPDATE_DELETE_LIMIT", None)
        .define("SQLITE_ENABLE_DBPAGE_VTAB", None)
        .define("SQLITE_ENABLE_STAT4", None)
        .warnings(false);
    if !cfg!(target_os = "windows") {
        sqlite_build.static_flag(true);
    }
    // Undefine FTS3 — use flag() since cc::Build has no .undefine() method.
    // The cc crate translates -U to /U on MSVC automatically.
    sqlite_build.flag("-USQLITE_ENABLE_FTS3");
    sqlite_build.compile("sqlite");

    // hopefully libsqlite3-sys finds this to fix windows builds?
    env::set_var("SQLITE3_LIB_DIR", env::var("OUT_DIR").unwrap());
    env::set_var("SQLITE3_INCLUDE_DIR", amalgamation_dir.clone());

    let extensions = vec![
        "base64", // "base85", // TODO re-add base85 at some point
        "decimal",
        "fileio",
        "ieee754",
        "sha1",
        "shathree",
        "spellfix",
        "series",
        "uuid",
        "completion",
        "uint",
    ];

    for ext in &extensions {
        build_sqlite_extension(ext, &sqlite_dir, &amalgamation_dir, c_opt_level, &[]);
    }

    // zlib-backed extensions (compress/uncompress, sqlar_compress/uncompress,
    // zipfile vtab). These #include <zlib.h>, so they need zlib's headers at
    // compile time and the zlib library at link time. libz-sys (a dependency
    // with the `static` feature) builds zlib from source on every platform —
    // including Windows/MSVC, where the old pkg-config zlib probe failed — and
    // exposes its header dir via DEP_Z_INCLUDE. libz-sys also emits the
    // link directive for zlib itself; cargo orders it after these archives
    // (which reference it), satisfying GNU ld's link-order requirement.
    let zlib_include = PathBuf::from(
        env::var("DEP_Z_INCLUDE")
            .expect("DEP_Z_INCLUDE not set — is libz-sys a dependency of solite-stdlib?"),
    );
    let zlib_extensions = ["compress", "sqlar", "zipfile"];
    for ext in &zlib_extensions {
        build_sqlite_extension(
            ext,
            &sqlite_dir,
            &amalgamation_dir,
            c_opt_level,
            std::slice::from_ref(&zlib_include),
        );
    }

    let mut build = cc::Build::new();
    build
        .file("./usleep.c")
        .warnings(false)
        .include(&amalgamation_dir)
        .opt_level(c_opt_level);
    if !cfg!(target_os = "windows") {
        build.static_flag(true);
    }
    if cfg!(feature = "static") {
        build.define("SQLITE_CORE", None);
    }
    build.compile("usleep");

    // Compile shell.c with main() renamed so we can call it from Rust.
    //
    // shell.c embeds inline copies of several extensions (shathree, sha1, uint,
    // decimal, ieee754, series, fileio, completion) that we also compile
    // separately above. macOS's linker tolerates duplicate symbols, but Linux's
    // rust-lld (used in GHA) does not. We rename the embedded copies via -D
    // defines so they don't clash — same trick used for main -> sqlite3_shell_main.
    let mut shell_build = cc::Build::new();
    shell_build
        .file(amalgamation_dir.join("shell.c"))
        .include(&amalgamation_dir)
        .opt_level(c_opt_level)
        .define("sqlite3_shathree_init", Some("_shell_shathree_init"))
        .define("sqlite3_sha_init", Some("_shell_sha_init"))
        .define("sqlite3_uint_init", Some("_shell_uint_init"))
        .define("sqlite3_decimal_init", Some("_shell_decimal_init"))
        .define("sqlite3_ieee_init", Some("_shell_ieee_init"))
        .define("sqlite3_series_init", Some("_shell_series_init"))
        .define("sqlite3_fileio_init", Some("_shell_fileio_init"))
        .define("sqlite3CompletionVtabInit", Some("_shell_CompletionVtabInit"))
        .define("sqlite3_base64_init", Some("_shell_base64_init"))
        .define("sqlite3_completion_init", Some("_shell_completion_init"))
        .warnings(false);
    if cfg!(target_os = "windows") {
        // On Windows, shell.c does `#define main utf8_main` internally,
        // so we must rename utf8_main instead of main.
        shell_build.define("utf8_main", Some("sqlite3_shell_main"));
    } else {
        shell_build
            .define("main", Some("sqlite3_shell_main"))
            .static_flag(true)
            .define("HAVE_READLINE", Some("1"))
            .define("HAVE_EDITLINE", Some("1"));
    }
    shell_build.compile("sqlite3_shell");

    // Compile sqldiff.c with main() renamed so we can call it from Rust
    let sqldiff_c = sqlite_dir.join("tool/sqldiff.c");
    let sqlite3_stdio_c = sqlite_dir.join("ext/misc/sqlite3_stdio.c");
    let sqlite3_stdio_dir = sqlite_dir.join("ext/misc");
    println!("cargo:rerun-if-changed={}", sqldiff_c.display());
    println!("cargo:rerun-if-changed={}", sqlite3_stdio_c.display());
    let mut sqldiff_build = cc::Build::new();
    sqldiff_build
        .file(&sqldiff_c)
        .file(&sqlite3_stdio_c)
        .include(&amalgamation_dir)
        .include(&sqlite3_stdio_dir)
        .opt_level(c_opt_level)
        .warnings(false);
    if cfg!(target_os = "windows") {
        // On Windows, sqldiff.c does `#define main utf8_main` internally,
        // so we must rename utf8_main instead of main.
        sqldiff_build.define("utf8_main", Some("sqldiff_main"));
    } else {
        sqldiff_build
            .define("main", Some("sqldiff_main"))
            .static_flag(true);
    }
    sqldiff_build.compile("sqldiff");

    // Compile sqlite3_rsync.c with main() renamed so we can call it from Rust
    let sqlite3_rsync_c = sqlite_dir.join("tool/sqlite3_rsync.c");
    println!("cargo:rerun-if-changed={}", sqlite3_rsync_c.display());
    let mut rsync_build = cc::Build::new();
    rsync_build
        .file(&sqlite3_rsync_c)
        .include(&amalgamation_dir)
        .opt_level(c_opt_level)
        .define("main", Some("sqlite3_rsync_main"))
        .warnings(false);
    if !cfg!(target_os = "windows") {
        rsync_build.static_flag(true);
    }
    rsync_build.compile("sqlite3_rsync");

    // Compile dbhash.c with main() renamed so we can call it from Rust.
    // Single file linked against the amalgamation; no sqlite3_stdio, so the
    // plain main -> dbhash_main rename works on every platform.
    //
    // dbhash.c declares a non-static global singleton named `g`, and so does
    // sqldiff.c. Both tools link into the single solite binary, so rename this
    // one to dbhash_g to avoid a duplicate-symbol link error (lld on Linux is
    // strict about this; macOS's linker is not). Every `g` token in this
    // translation unit is the singleton or a `g.field` access, so the rename
    // is consistent and self-contained.
    let dbhash_c = sqlite_dir.join("tool/dbhash.c");
    println!("cargo:rerun-if-changed={}", dbhash_c.display());
    let mut dbhash_build = cc::Build::new();
    dbhash_build
        .file(&dbhash_c)
        .include(&amalgamation_dir)
        .opt_level(c_opt_level)
        .define("main", Some("dbhash_main"))
        .define("g", Some("dbhash_g"))
        .warnings(false);
    if !cfg!(target_os = "windows") {
        dbhash_build.static_flag(true);
    }
    dbhash_build.compile("dbhash");

    // Compile dbtotxt.c with main() renamed so we can call it from Rust.
    // dbtotxt reads the database file's raw bytes and links no sqlite3
    // symbols; the include path is harmless. No sqlite3_stdio, so the plain
    // main -> dbtotxt_main rename works on every platform.
    let dbtotxt_c = sqlite_dir.join("tool/dbtotxt.c");
    println!("cargo:rerun-if-changed={}", dbtotxt_c.display());
    let mut dbtotxt_build = cc::Build::new();
    dbtotxt_build
        .file(&dbtotxt_c)
        .include(&amalgamation_dir)
        .opt_level(c_opt_level)
        .define("main", Some("dbtotxt_main"))
        .warnings(false);
    if !cfg!(target_os = "windows") {
        dbtotxt_build.static_flag(true);
    }
    dbtotxt_build.compile("dbtotxt");

    // Compile sqlite3_expert: expert.c provides main() (renamed), and
    // sqlite3expert.c is the analysis engine. Both link against the
    // amalgamation; ext/expert is on the include path for sqlite3expert.h.
    // No sqlite3_stdio, so the plain main -> sqlite3_expert_main rename works
    // on every platform.
    let expert_dir = sqlite_dir.join("ext/expert");
    let expert_c = expert_dir.join("expert.c");
    let sqlite3expert_c = expert_dir.join("sqlite3expert.c");
    println!("cargo:rerun-if-changed={}", expert_c.display());
    println!("cargo:rerun-if-changed={}", sqlite3expert_c.display());
    println!(
        "cargo:rerun-if-changed={}",
        expert_dir.join("sqlite3expert.h").display()
    );
    let mut expert_build = cc::Build::new();
    expert_build
        .file(&expert_c)
        .file(&sqlite3expert_c)
        .include(&amalgamation_dir)
        .include(&expert_dir)
        .opt_level(c_opt_level)
        .define("main", Some("sqlite3_expert_main"))
        .warnings(false);
    if !cfg!(target_os = "windows") {
        expert_build.static_flag(true);
    }
    expert_build.compile("sqlite3_expert");

    // Link libedit (macOS system editline) or readline for the sqlite3 shell.
    // On Windows, the shell works without readline/editline.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=edit");
    } else if !cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=readline");
    }

    // rerun-if-changed for extension source files
    for ext in extensions.iter().chain(zlib_extensions.iter()) {
        println!(
            "cargo:rerun-if-changed={}",
            sqlite_dir.join(format!("ext/misc/{ext}.c")).display()
        );
    }
    println!("cargo:rerun-if-changed=usleep.c");
    println!("cargo:rerun-if-changed=build.rs");
}
