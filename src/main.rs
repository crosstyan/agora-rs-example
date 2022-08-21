extern crate dirs;
use agora_rtsa_rs::agoraRTC;
use agora_rtsa_rs::agoraRTC::LogLevel;
use agora_rtsa_rs::C;
use log::{error, info, warn};
use std::env;
use std::ffi::CString;
use std::thread::sleep;

fn result_verify(res: Result<(), agoraRTC::ErrorCode>, action_name: &str) {
    match res {
        Ok(()) => info!("{} successfully", action_name),
        Err(err) => {
            let re = agoraRTC::err_2_reason(err);
            error!("{} failed! Reason: {}", action_name, re);
            panic!();
        }
    }
}

fn main() {
    let home = dirs::home_dir().unwrap();
    // the certificate should be located in your home dir
    let cert = std::fs::read_to_string(home.join("certificate.bin")).unwrap();
    let app_id = "3759fd9101e04094869e7e69b9b3fe64";
    let channel_name = "test";
    let app_token = "007eJxTYLiz0kH/x3WBX3dO/GMTN3rG+cVYYduUmhwuppt8h85aPVRUYDA2N7VMS7E0NDBMNTAxsDSxMLNMNU81s0yyTDJOSzUz2X6EMdlMjDn5+foNLIwMEAjiszCUpBaXMDAAAGYbH8k=";
    let app_token_c = CString::new(app_token).expect("CString::new error");
    let log_path = "logs";
    let log_path_c = CString::new(log_path).unwrap();
    let uid: u32 = 1234;

    // Set the default log level to debug
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    // Output the log to STDOUT
    let mut builder = env_logger::Builder::from_default_env();
    builder.target(env_logger::Target::Stdout);
    builder.init();

    info!("Agora Version: {}", agoraRTC::get_version());
    result_verify(agoraRTC::license_verify(cert.as_str()), "license_verify");

    let handlers = C::agora_rtc_event_handler_t::new();
    let log_cfg = agoraRTC::LogConfig {
        log_disable: false,
        log_disable_desensitize: true,
        log_level: LogLevel::DEBUG,
    };

    let opt = agoraRTC::RtcServiceOption {
        area_code: agoraRTC::AreaCode::CN,
        product_id: [0; 64],
        log_cfg: log_cfg,
        license_value: [0; 33],
    };

    // You have to set logs before deallocation
    let opt_t: C::rtc_service_option_t = opt.to_c_type(log_path_c.as_ptr());

    result_verify(agoraRTC::init(app_id, opt_t, handlers), "init");
    let conn_id: u32 = match agoraRTC::create_connection() {
        Ok(id) => id,
        Err(code) => {
            let reason = agoraRTC::err_2_reason(code);
            panic!("create_connection failed. Reason: {}", reason);
        }
    };
    let chan_opt = C::rtc_channel_options_t::new();
    result_verify(
        agoraRTC::join_channel(
            conn_id,
            channel_name,
            Some(uid),
            app_token_c.as_ptr(),
            chan_opt,
        ),
        "join channel",
    );
    sleep(std::time::Duration::from_millis(5000));
    result_verify(agoraRTC::leave_channel(conn_id), "leave channel");
    result_verify(agoraRTC::destroy_connection(conn_id), "destory connection");
    result_verify(agoraRTC::deinit(), "dinit")
}
