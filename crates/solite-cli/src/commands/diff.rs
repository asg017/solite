use std::ffi::CString;
use std::os::raw::{c_char, c_int};

extern "C" {
    fn sqldiff_main(argc: c_int, argv: *mut *mut c_char) -> c_int;
}

pub fn diff(args: Vec<String>) -> Result<(), ()> {
    let mut full_args = vec!["sqldiff".to_string()];
    full_args.extend(args);

    let c_args: Vec<CString> = full_args
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();
    let mut c_argv: Vec<*mut c_char> = c_args.iter().map(|s| s.as_ptr() as *mut c_char).collect();

    let rc = unsafe { sqldiff_main(c_argv.len() as c_int, c_argv.as_mut_ptr()) };

    if rc == 0 {
        Ok(())
    } else {
        Err(())
    }
}
