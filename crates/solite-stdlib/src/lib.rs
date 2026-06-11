mod ext;

use crate::ext::sqlite3_solite_stdlib_init;
use sqlite_http::sqlite3_http_init;
use sqlite_loadable::ext::{sqlite3, sqlite3_api_routines};
use sqlite_path::sqlite3_path_init;
use sqlite_regex::sqlite3_regex_init;
use sqlite_str::sqlite3_str_init;
use sqlite_ulid::sqlite3_ulid_init;
use sqlite_url::sqlite3_url_init;
// sqlite-vec declares its own `sqlite3_vec_init` as a zero-arg fn; the true
// extern is declared below. This import keeps the crate (and its compiled C
// library) linked.
use sqlite_vec as _;
use sqlite_xsv::sqlite3_xsv_init;
use std::ffi::{c_char, c_int, c_uint};

include!(concat!(env!("OUT_DIR"), "/builtins.rs"));

// C extension entry points, compiled by build.rs from vendor/sqlite/ext/misc/
// (usleep.c is local, sqlite-vec.c comes from the sqlite-vec crate). All share
// the standard SQLite extension signature:
//   int sqlite3_X_init(sqlite3*, char**, const sqlite3_api_routines*)
#[link(name = "base64")]
extern "C" {
    fn sqlite3_base64_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "decimal")]
extern "C" {
    fn sqlite3_decimal_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "ieee754")]
extern "C" {
    fn sqlite3_ieee_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "fileio")]
extern "C" {
    fn sqlite3_fileio_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "uint")]
extern "C" {
    fn sqlite3_uint_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "series")]
extern "C" {
    fn sqlite3_series_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "sha1")]
extern "C" {
    fn sqlite3_sha_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "shathree")]
extern "C" {
    fn sqlite3_shathree_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "spellfix")]
extern "C" {
    fn sqlite3_spellfix_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "uuid")]
extern "C" {
    fn sqlite3_uuid_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "usleep")]
extern "C" {
    fn sqlite3_usleep_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
#[link(name = "completion")]
extern "C" {
    fn sqlite3_completion_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}
extern "C" {
    // linked via the sqlite-vec crate's build script, not #[link]
    fn sqlite3_vec_init(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *const sqlite3_api_routines,
    ) -> c_int;
}

// Intentionally not bundled: base85 (niche encoding), sqlar/compress/zipfile
// (need zlib), fastrand (unmaintained). A future "safe mode" build could also
// exclude the filesystem/network/sleep extensions (http, fileio, usleep).

/// Returns early with the rc as `c_uint` if an init call failed. Accepts both
/// the `c_int` C extensions and the `c_uint` sqlite-loadable entry points.
macro_rules! try_init {
    ($call:expr) => {
        let rc = $call;
        if rc != 0 {
            return rc as c_uint;
        }
    };
}

/// Registers every bundled extension on `db`. Returns `SQLITE_OK` (0) on
/// success, or the first non-OK result code from an extension's init function
/// (remaining extensions are skipped).
///
/// # Safety
///
/// - `db` must be a valid, open `sqlite3` connection handle.
/// - `pz_err_msg` may be null; if non-null it must be a valid place for an
///   extension to store an error-message pointer (standard SQLite extension
///   contract — the caller frees it with `sqlite3_free`).
/// - `p_api` may be null: extensions here are compiled with `SQLITE_CORE`, so
///   they call SQLite directly rather than through the api-routines table.
/// - Calling more than once on the same connection is safe: functions and
///   modules are re-registered, replacing the previous registration.
#[no_mangle]
pub unsafe extern "C" fn solite_stdlib_init(
    db: *mut sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut sqlite3_api_routines,
) -> c_uint {
    try_init!(sqlite3_ulid_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_regex_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_http_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_path_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_url_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_xsv_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_str_init(db, pz_err_msg, p_api));

    try_init!(sqlite3_solite_stdlib_init(db, pz_err_msg, p_api));

    try_init!(sqlite3_base64_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_vec_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_completion_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_decimal_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_ieee_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_fileio_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_uint_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_series_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_sha_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_shathree_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_spellfix_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_uuid_init(db, pz_err_msg, p_api));
    try_init!(sqlite3_usleep_init(db, pz_err_msg, p_api));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn functions_of(db: Connection) -> Vec<String> {
        db.prepare("select distinct name from pragma_function_list order by 1")
            .unwrap()
            .query_map([], |row| Ok(row.get::<usize, String>(0).unwrap()))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
    #[test]
    fn stdlib_basic() {
        let db = Connection::open_in_memory().unwrap();

        // Don't snapshot the version itself — it changes every SQLite bump.
        // The canonical version assertion lives in solite-core's sqlite_version snapshot.
        let version: String = db
            .query_row("select sqlite_version();", [], |r| r.get(0))
            .unwrap();
        assert!(
            version.split('.').count() == 3 && version.split('.').all(|p| p.parse::<u32>().is_ok()),
            "unexpected sqlite_version: {version}"
        );

        let base_functions: Vec<String> = functions_of(db);
        insta::assert_yaml_snapshot!(base_functions);

        let db = Connection::open_in_memory().unwrap();
        unsafe {
            solite_stdlib_init(db.handle(), std::ptr::null_mut(), std::ptr::null_mut());
        }
        /*assert!(db
        .query_row("select ulid_version();", [], |r| r.get::<usize, String>(0))
        .unwrap()
        .starts_with('v'));*/
        let solite_functions: Vec<String> = functions_of(db);
        let solite_only_functions: Vec<String> = solite_functions
            .iter()
            .filter(|v| !base_functions.contains(v))
            .map(|v| v.to_string())
            .collect::<Vec<String>>();
        insta::assert_yaml_snapshot!(solite_only_functions);

        insta::assert_yaml_snapshot!(BUILTIN_FUNCTIONS);
    }

    #[test]
    fn stdlib_double_init() {
        let db = Connection::open_in_memory().unwrap();
        unsafe {
            assert_eq!(
                solite_stdlib_init(db.handle(), std::ptr::null_mut(), std::ptr::null_mut()),
                0
            );
            // re-init on the same connection re-registers everything and still succeeds
            assert_eq!(
                solite_stdlib_init(db.handle(), std::ptr::null_mut(), std::ptr::null_mut()),
                0
            );
        }
        let v: String = db
            .query_row("select solite_stdlib_version();", [], |r| r.get(0))
            .unwrap();
        assert!(!v.is_empty());
    }
}
