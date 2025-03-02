use pkg_config::Library;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "3.47.0";
const AMALGAMATION: (&str, &str) = ("2024", "3470000");

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
        "pub static BUILTIN_FUNCTIONS: &'static [&'static str] = &{:?};",
        functions
    )
    .unwrap();
}

fn build_sqlite_org_extension(
    name: &str,
    out_dir: &Path,
    amalgammation_src_dir: &PathBuf,
    //zlib: Library,
) {
    let c_file = format!("{name}.c");
    let srcdir = out_dir.join(format!("sqlite.org-source-{VERSION}"));
    std::fs::create_dir_all(&srcdir).unwrap();
    let c_file_path = srcdir.join(&c_file);
    if !c_file_path.exists() {
        let status = Command::new("curl")
            .arg("-L")
            .arg(format!(
                "https://github.com/sqlite/sqlite/raw/version-{VERSION}/ext/misc/{}",
                &c_file,
            ))
            .arg("-o")
            .arg(&c_file_path)
            .status()
            .unwrap();

        if !status.success() {
            panic!("Failed to download {c_file}");
        }
    }

    let mut build = cc::Build::new();
    build
        .file(&c_file_path)
        .flag("-O3")
        .warnings(false)
        .include(amalgammation_src_dir);
        //.includes(zlib.include_paths);
    if cfg!(feature = "static") {
        build.define("SQLITE_CORE", None);
    }
    if name == "fileio" && cfg!(target_os = "windows") {
        build.file(amalgammation_src_dir.join("test_windirent.c"));
    }
    build.compile(name);
}

fn main() {
    generate_builtins();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let amalgammation_dir = out_dir.join("amalgamation");
    let amalgammation_src_dir =
        amalgammation_dir.join(format!("sqlite-amalgamation-{}", AMALGAMATION.1));

    if !amalgammation_dir.exists() {
        let zip_path = out_dir.join(format!("amalgamation.{VERSION}.zip"));

        let status = Command::new("curl")
            .arg("-L")
            .arg(format!(
                "https://www.sqlite.org/{}/sqlite-amalgamation-{}.zip",
                AMALGAMATION.0, AMALGAMATION.1
            ))
            .arg("-o")
            .arg(&zip_path)
            .status()
            .unwrap();

        if !status.success() {
            panic!("Failed to download amalgammation");
        }

        let status = Command::new("unzip")
            .arg(&zip_path)
            .arg("-d")
            .arg(&amalgammation_dir)
            .status()
            .unwrap();

        if !status.success() {
            panic!("Failed to unzip amalgammation");
        }
        std::fs::remove_file(zip_path).unwrap();
    }

    cc::Build::new()
        .file(amalgammation_src_dir.join("sqlite3.c"))
        .include(&amalgammation_src_dir)
        .static_flag(true)
        .opt_level(3)
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
        .warnings(false)
        .compile("sqlite");

    let sqlite_org_extensions = vec![
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

    if cfg!(target_os = "windows") {
        let status = Command::new("curl")
            .arg("-L")
            .arg(format!(
                "https://github.com/sqlite/sqlite/raw/version-{VERSION}/src/test_windirent.h",
            ))
            .arg("-o")
            .arg(amalgammation_src_dir.join("test_windirent.h"))
            .status()
            .unwrap();

        if !status.success() {
            panic!("Failed to download test_windirent.h");
        }
        let status = Command::new("curl")
            .arg("-L")
            .arg(format!(
                "https://github.com/sqlite/sqlite/raw/version-{VERSION}/src/test_windirent.c",
            ))
            .arg("-o")
            .arg(amalgammation_src_dir.join("test_windirent.c"))
            .status()
            .unwrap();

        if !status.success() {
            panic!("Failed to download test_windirent.h");
        }
    }

    //let zlib = pkg_config::probe_library("zlib").unwrap();

    for ext in sqlite_org_extensions {
        build_sqlite_org_extension(ext, &out_dir, &amalgammation_src_dir /*, zlib.clone() */);
    }

    let mut build = cc::Build::new();
    build
        .file("./usleep.c")
        .warnings(false)
        .include(&amalgammation_src_dir)
        .opt_level(3);
    if !cfg!(target_os = "windows") {
        build.static_flag(true);
    }
    if cfg!(feature = "static") {
        build.define("SQLITE_CORE", None);
    }
    build.compile("usleep");

    println!("cargo:rerun-if-changed=usleep.c");
    println!("cargo:rerun-if-changed=build.rs");

    // breaks for macos - but works for linux??
    /*
    if cfg!(target_os = "linux") {
        println!("cargo::rustc-link-lib=static=z");
    }
     */
}
