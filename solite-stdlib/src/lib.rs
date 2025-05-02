mod ext;

use crate::ext::sqlite3_solite_stdlib_init;
use sqlite_http::sqlite3_http_init;
use sqlite_loadable::ext::{sqlite3, sqlite3_api_routines};
use sqlite_path::sqlite3_path_init;
use sqlite_regex::sqlite3_regex_init;
use sqlite_ulid::sqlite3_ulid_init;
use sqlite_url::sqlite3_url_init;
use sqlite_vec::sqlite3_vec_init;
use std::ffi::{c_char, c_uint};
//use sqlite_fastrand::sqlite3_fastrand_init;
use sqlite_xsv::sqlite3_xsv_init;

include!(concat!(env!("OUT_DIR"), "/builtins.rs"));

#[link(name = "base64")]
extern "C" {
    fn sqlite3_base_init();
}
//#[link(name = "base85")]
//extern "C" {
//    fn sqlite3_base85_init();
//}
#[link(name = "decimal")]
extern "C" {
    fn sqlite3_decimal_init();
}
#[link(name = "ieee754")]
extern "C" {
    fn sqlite3_ieee_init();
}
#[link(name = "fileio")]
extern "C" {
    fn sqlite3_fileio_init();
}
#[link(name = "uint")]
extern "C" {
    fn sqlite3_uint_init();
}
#[link(name = "series")]
extern "C" {
    fn sqlite3_series_init();
}
#[link(name = "sha1")]
extern "C" {
    fn sqlite3_sha_init();
}
#[link(name = "shathree")]
extern "C" {
    fn sqlite3_shathree_init();
}
#[link(name = "spellfix")]
extern "C" {
    fn sqlite3_spellfix_init();
}

/*
#[link(name = "sqlar")]
extern "C" {
    fn sqlite3_sqlar_init();
}
#[link(name = "compress")]
extern "C" {
    fn sqlite3_compress_init();
}
*/
#[link(name = "uuid")]
extern "C" {
    fn sqlite3_uuid_init();
}

/*
#[link(name = "zipfile")]
extern "C" {
    fn sqlite3_zipfile_init();
}
*/

#[link(name = "usleep")]
extern "C" {
    fn sqlite3_usleep_init();
}

#[link(name = "completion")]
extern "C" {
    fn sqlite3_completion_init();
}

unsafe fn init_arg0(
    func: unsafe extern "C" fn(),
    db: *mut sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut sqlite3_api_routines,
) {
    let x: unsafe extern "C" fn(
        db: *mut sqlite3,
        pz_err_msg: *mut *mut c_char,
        p_api: *mut sqlite3_api_routines,
    ) -> c_uint = std::mem::transmute(func as *const ());
    x(db, pz_err_msg, p_api);
}
/// # Safety
/// lol
#[no_mangle]
pub unsafe extern "C" fn solite_stdlib_init(
    db: *mut sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut sqlite3_api_routines,
) -> c_uint {
    sqlite3_ulid_init(db, pz_err_msg, p_api);
    sqlite3_regex_init(db, pz_err_msg, p_api);
    sqlite3_http_init(db, pz_err_msg, p_api);
    sqlite3_path_init(db, pz_err_msg, p_api);
    sqlite3_url_init(db, pz_err_msg, p_api);
    sqlite3_xsv_init(db, pz_err_msg, p_api);

    sqlite3_solite_stdlib_init(db, pz_err_msg, p_api);

    init_arg0(sqlite3_base_init, db, pz_err_msg, p_api);

    //sqlite3_fastrand_init(db, pz_err_msg, p_api);
    init_arg0(sqlite3_vec_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_completion_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_decimal_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_ieee_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_fileio_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_uint_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_series_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_sha_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_shathree_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_spellfix_init, db, pz_err_msg, p_api);
    //init_arg0(sqlite3_sqlar_init, db, pz_err_msg, p_api);
    //init_arg0(sqlite3_compress_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_uuid_init, db, pz_err_msg, p_api);
    init_arg0(sqlite3_usleep_init, db, pz_err_msg, p_api);
    //init_arg0(sqlite3_zipfile_init, db, pz_err_msg, p_api);
    //init_arg0(sqlite3_base85_init, db, pz_err_msg, p_api);
    0
}

// TODO different modes
// safe mode: HTTP, usleep, zipfile, fileio

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

        assert_eq!(
            db.query_row("select sqlite_version();", [], |r| r
                .get::<usize, String>(0))
                .unwrap(),
            "3.49.1"
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
}
