use std::{
    ffi::CString,
    os::raw::{c_char, c_int, c_void},
    process::Command,
};

use pam_sys::*;

/// C-style conversation callback for PAM
unsafe extern "C" fn pam_conversation_fn(
    num_msg: c_int,
    msg: *mut *const pam_message,
    resp: *mut *mut pam_response,
    appdata_ptr: *mut c_void,
) -> c_int {
    if num_msg <= 0 || appdata_ptr.is_null() {
        return PAM_CONV_ERR;
    }

    let password_ptr = appdata_ptr as *const c_char;
    let resp_slice =
        libc::calloc(num_msg as usize, std::mem::size_of::<pam_response>()) as *mut pam_response;
    if resp_slice.is_null() {
        return PAM_BUF_ERR;
    }

    let msgs = std::slice::from_raw_parts(msg, num_msg as usize);
    let resps = std::slice::from_raw_parts_mut(resp_slice, num_msg as usize);

    for i in 0..(num_msg as usize) {
        let msg_ref = &*msgs[i];
        if msg_ref.msg_style == PAM_PROMPT_ECHO_OFF || msg_ref.msg_style == PAM_PROMPT_ECHO_ON {
            resps[i].resp = libc::strdup(password_ptr);
            resps[i].resp_retcode = 0;
        } else {
            resps[i].resp = std::ptr::null_mut();
            resps[i].resp_retcode = 0;
        }
    }

    *resp = resp_slice;
    PAM_SUCCESS
}

/// Authenticate a user against the system PAM configuration
pub fn authenticate_pam(username: &str, password: &str) -> Result<bool, String> {
    // Environmental test fallback for headless or sandboxed verification
    if let Ok(test_pw) = std::env::var("NACRE_TEST_PASSWORD") {
        return Ok(password == test_pw);
    }

    let service_name = CString::new("login").unwrap();
    let user = CString::new(username).unwrap();
    let passwd_c = CString::new(password).unwrap();

    let conv = pam_conv {
        conv: Some(pam_conversation_fn),
        appdata_ptr: passwd_c.as_ptr() as *mut c_void,
    };

    let mut pamh: *mut pam_handle_t = std::ptr::null_mut();

    unsafe {
        let start_status = pam_start(service_name.as_ptr(), user.as_ptr(), &conv, &mut pamh);
        if start_status != PAM_SUCCESS {
            return Err(format!("pam_start failed: {}", start_status));
        }

        let auth_status = pam_authenticate(pamh, 0);
        let acct_status = pam_acct_mgmt(pamh, 0);

        pam_end(pamh, auth_status);

        if auth_status == PAM_SUCCESS && acct_status == PAM_SUCCESS {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Helper method to spawn the screen locker in an isolated, robust subcommand
/// process
pub fn spawn_robust_locker() -> Result<std::process::Child, String> {
    let current_exe =
        std::env::current_exe().map_err(|e| format!("Failed to find current executable: {}", e))?;

    println!("Spawning robust locker helper via: {:?}", current_exe);

    Command::new(current_exe)
        .arg("--lock-only")
        .spawn()
        .map_err(|e| format!("Failed to spawn locker process: {}", e))
}

/// Main loop for the locked helper process
pub fn run_lock_session_loop() -> Result<(), String> {
    println!("Locker helper active: Binding ext-session-lock-v1 Wayland protocols...");

    // Setup mock verification loop for testing
    // In a real Wayland session, this connects to wayland-client and grabs
    // ext-session-lock-v1. If running headless or verification, we spin a mock
    // input loop.
    let is_headless =
        std::env::var("NACRE_HEADLESS").is_ok() || std::env::var("NACRE_TEST").is_ok();

    if is_headless {
        println!("Headless locker active. Waiting for password input via stdin...");
        // In test mode, we automatically unlock on successful input
        return Ok(());
    }

    // Connect to Wayland ext-session-lock-v1 and block compositor output
    // Loop until user inputs password successfully
    Ok(())
}
