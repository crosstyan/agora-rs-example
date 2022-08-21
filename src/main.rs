extern crate dirs;
use agora_rtsa_rs::agoraRTC;
use agora_rtsa_rs::agoraRTC::{LogLevel, VideoDataType, VideoFrameType, VideoStreamQuality};
use agora_rtsa_rs::C::{self, video_data_type_e_VIDEO_DATA_TYPE_H264};
use anyhow::{anyhow, bail, Context};
use gst::element_error;
use gst::glib;
use gst::glib::closure;
use gst::prelude::*;
use gst_app::{AppSink, AppSrc};
use log::{error, info, warn};
use std::env;
use std::ffi::{CString, c_void};
use std::thread::sleep;
use std::sync::{Arc, Mutex};


// Check if all GStreamer plugins we require are available
fn check_plugins() -> Result<(), anyhow::Error> {
    let needed = [
        "videotestsrc",
        "audiotestsrc",
        "videoconvert",
        "audioconvert",
        "autodetect",
        "clockoverlay",
        "videoscale",
        "x264enc",
        "h264parse",
        "appsink",
    ];

    let registry = gst::Registry::get();

    // export GST_PLUGIN_PATH=/opt/homebrew/lib/gstreamer-1.0
    // to find all plugins for macOS
    let missing = needed
        .iter()
        .filter(|n| registry.find_plugin(n).is_none())
        .cloned()
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        bail!("Missing plugins: {:?}", missing);
    } else {
        Ok(())
    }
}

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

    gst::init();
    check_plugins();

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

    /// If frame_per_sec equals zero, then real timestamp will be used. So I don't need to calculate them.
    let video_opt = C::video_frame_info_t {
        data_type: VideoDataType::H264.into(),
        stream_type: VideoStreamQuality::LOW.into(),
        frame_type: VideoFrameType::AUTO.into(),
        frame_rate: 0,
    };

    // Create the GStreamer pipeline
    let pipeline = gst::parse_launch(
        "videotestsrc name=src is-live=true ! \
        clockoverlay ! \
        videoconvert ! \
        x264enc speed-preset=ultrafast tune=zerolatency byte-stream=true intra-refresh=true ! \
        h264parse ! \
        appsink name=agora ",
    )
    .expect("not a elem");

    // Downcast from gst::Element to gst::Pipeline
    let pipeline = pipeline
        .downcast::<gst::Pipeline>()
        .expect("not a pipeline");
    // let source = pipeline.by_name("src").expect("can't find src");
    // Get access to the webrtcbin by name
    let appsink = pipeline
        .by_name("agora")
        .expect("can't find agora")
        .dynamic_cast::<AppSink>()
        .expect("should be an appsink");
    // https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/-/blob/main/tutorials/src/bin/basic-tutorial-8.rs

    result_verify(agoraRTC::init(app_id, opt_t, handlers), "init");
    let conn_id: u32 = match agoraRTC::create_connection() {
        Ok(id) => id,
        Err(code) => {
            let reason = agoraRTC::err_2_reason(code);
            panic!("create_connection failed. Reason: {}", reason);
        }
    };

    appsink.clone().set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |_| {
                if let Ok(sample) = appsink.pull_sample() {
                    let mem = Arc::new(sample.buffer().unwrap());
                    unsafe{
                        if mem.as_ptr() != std::ptr::null() {
                            let ptr = &video_opt as *const C::video_frame_info_t as *mut C::video_frame_info_t;
                            C::agora_rtc_send_video_data(conn_id, mem.as_ptr() as *const c_void, mem.size().try_into().unwrap(), ptr);
                        }
                    }
                    use std::io::{self, Write};
                    // The only thing we do in this example is print a * to indicate a received buffer
                    print!("*");
                    let _ = io::stdout().flush();
                }

                Ok(gst::FlowSuccess::Ok)
            })
            .build()
    );
    // Create a stream for handling the GStreamer message asynchronously
    // let bus = pipeline.bus().unwrap();
    // let send_gst_msg_rx = bus.stream();
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
    pipeline
        .set_state(gst::State::Playing)
        .expect("set playing error");
    sleep(std::time::Duration::from_millis(200000));
    pipeline
        .set_state(gst::State::Paused)
        .expect("set state error");
    result_verify(agoraRTC::leave_channel(conn_id), "leave channel");
    result_verify(agoraRTC::destroy_connection(conn_id), "destory connection");
    result_verify(agoraRTC::deinit(), "dinit")
}
