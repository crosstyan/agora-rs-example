extern crate dirs;
use agora_rtsa_rs::agoraRTC::LogLevel;
use agora_rtsa_rs::{agoraRTC};
use agora_rtsa_rs::C;
use std::env;
use log::{error, info, warn};

fn result_verify(res: Result<(), agoraRTC::ErrorCode>, action_name: &str){
    match res {
        Ok(()) => info!("{} successfully", action_name),
        Err(err) => {
            let re = agoraRTC::err_2_reason(err);
            error!("{} failed! Reason: {}", action_name,re);
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
    env_logger::init();
    // Set the default log level to debug
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }

    println!("Agora Version: {}", agoraRTC::get_version());
    result_verify(agoraRTC::license_verify(cert.as_str()), "license_verify");
    let handlers = C::agora_rtc_event_handler_t::new();
    let log_cfg = agoraRTC::LogConfig{
        log_disable: false,
        log_disable_desensitize: false,
        log_level: LogLevel::DEBUG,
        log_path: home.join("agora.log").into_os_string().into_string().unwrap(),
    };

    // I have no idea
    let opt = agoraRTC::RtcServiceOption{
        area_code: agoraRTC::AreaCode::CN,
        product_id: [0;64],
        log_cfg: log_cfg,
        license_value: [0;33],
    };
    result_verify(agoraRTC::init(app_id, opt, handlers), "init");
    result_verify(agoraRTC::deinit(), "dinit")
}
