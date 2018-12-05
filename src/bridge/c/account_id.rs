use crate::AccountId;
use libc::c_char;
use std::ffi::CStr;

#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn hedera_account_id_from_str(s: *const c_char, out: *mut AccountId) -> u64 {
    debug_assert!(!s.is_null());
    debug_assert!(!out.is_null());

    let s = CStr::from_ptr(s);
    let s = s.to_string_lossy();

    *out = try_ffi!(s.parse());

    0
}
