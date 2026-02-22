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

        // sqlite3.h must be built before sqlite3.c due to Makefile dependency ordering
        let status = Command::new("make")
            .arg("sqlite3.h")
            .current_dir(sqlite_dir)
            .status()
            .expect("Failed to run make sqlite3.h in sqlite submodule");
        if !status.success() {
            panic!("make sqlite3.h failed in sqlite submodule");
        }

        let status = Command::new("make")
            .arg("sqlite3.c")
            .current_dir(sqlite_dir)
            .status()
            .expect("Failed to run make sqlite3.c in sqlite submodule");
        if !status.success() {
            panic!("make sqlite3.c failed in sqlite submodule");
        }
    }
    sqlite_dir.to_path_buf()
}

fn build_sqlite_extension(
    name: &str,
    sqlite_dir: &Path,
    amalgamation_dir: &Path,
    c_opt_level: u32,
) {
    let c_file = sqlite_dir.join(format!("ext/misc/{name}.c"));
    let mut build = cc::Build::new();
    build
        .file(&c_file)
        .opt_level(c_opt_level)
        .warnings(false)
        .include(amalgamation_dir);
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
        /* temporary, until windows zlib stuff is fixed
        "compress",
        "zipfile", //"stmt",
        "sqlar",
         */
        "series",
        "uuid",
        "completion",
        "uint",
    ];

    for ext in &extensions {
        build_sqlite_extension(ext, &sqlite_dir, &amalgamation_dir, c_opt_level);
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

    // Link libedit (macOS system editline) or readline for the sqlite3 shell.
    // On Windows, the shell works without readline/editline.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=edit");
    } else if !cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=readline");
    }

    // rerun-if-changed for extension source files
    for ext in &extensions {
        println!(
            "cargo:rerun-if-changed={}",
            sqlite_dir.join(format!("ext/misc/{ext}.c")).display()
        );
    }
    println!("cargo:rerun-if-changed=usleep.c");
    println!("cargo:rerun-if-changed=build.rs");
}
