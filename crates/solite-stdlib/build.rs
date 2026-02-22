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
        let status = Command::new("./configure")
            .current_dir(sqlite_dir)
            .status()
            .expect("Failed to run ./configure in sqlite submodule");
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
        let windirent_c = sqlite_dir.join("src/test_windirent.c");
        let windirent_h = sqlite_dir.join("src/test_windirent.h");
        // Copy test_windirent.h to amalgamation dir so fileio.c can find it
        std::fs::copy(&windirent_h, amalgamation_dir.join("test_windirent.h")).unwrap();
        build.file(windirent_c);
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
    cc::Build::new()
        .file(amalgamation_dir.join("sqlite3.c"))
        .include(&amalgamation_dir)
        .static_flag(true)
        .opt_level(c_opt_level)
        .flag("-DSQLITE_ENABLE_RTREE")
        .flag("-DSQLITE_SOUNDEX")
        .flag("-DSQLITE_ENABLE_GEOPOLY")
        .flag("-DSQLITE_ENABLE_MATH_FUNCTIONS")
        .flag("-USQLITE_ENABLE_FTS3")
        .flag("-DSQLITE_ENABLE_FTS5")
        .flag("-DSQLITE_ENABLE_DBSTAT_VTAB")
        .flag("-DSQLITE_ENABLE_STMTVTAB")
        .flag("-DSQLITE_ENABLE_BYTECODE_VTAB")
        .flag("-DSQLITE_ENABLE_EXPLAIN_COMMENTS")
        .flag("-DSQLITE_ENABLE_STMT_SCANSTATUS")
        .flag("-DSQLITE_ENABLE_COLUMN_METADATA ")
        .warnings(false)
        .compile("sqlite");

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

    // Compile shell.c with main() renamed so we can call it from Rust
    cc::Build::new()
        .file(amalgamation_dir.join("shell.c"))
        .include(&amalgamation_dir)
        .static_flag(true)
        .opt_level(c_opt_level)
        .define("main", Some("sqlite3_shell_main"))
        .define("HAVE_READLINE", Some("1"))
        .define("HAVE_EDITLINE", Some("1"))
        .warnings(false)
        .compile("sqlite3_shell");

    // Compile sqldiff.c with main() renamed so we can call it from Rust
    let sqldiff_c = sqlite_dir.join("tool/sqldiff.c");
    let sqlite3_stdio_c = sqlite_dir.join("ext/misc/sqlite3_stdio.c");
    let sqlite3_stdio_dir = sqlite_dir.join("ext/misc");
    println!("cargo:rerun-if-changed={}", sqldiff_c.display());
    println!("cargo:rerun-if-changed={}", sqlite3_stdio_c.display());
    cc::Build::new()
        .file(&sqldiff_c)
        .file(&sqlite3_stdio_c)
        .include(&amalgamation_dir)
        .include(&sqlite3_stdio_dir)
        .static_flag(true)
        .opt_level(c_opt_level)
        .define("main", Some("sqldiff_main"))
        .warnings(false)
        .compile("sqldiff");

    // Compile sqlite3_rsync.c with main() renamed so we can call it from Rust
    let sqlite3_rsync_c = sqlite_dir.join("tool/sqlite3_rsync.c");
    println!("cargo:rerun-if-changed={}", sqlite3_rsync_c.display());
    cc::Build::new()
        .file(&sqlite3_rsync_c)
        .include(&amalgamation_dir)
        .static_flag(true)
        .opt_level(c_opt_level)
        .define("main", Some("sqlite3_rsync_main"))
        .warnings(false)
        .compile("sqlite3_rsync");

    // Link libedit (macOS system editline) or readline for the sqlite3 shell
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=edit");
    } else {
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
