extern crate dirs;
use agora_rtsa_rs::agoraRTC;
use agora_rtsa_rs::agoraRTC::{LogLevel, VideoDataType, VideoFrameType, VideoStreamQuality};
use agora_rtsa_rs::C::{self};
use anyhow::{bail};
use serde_derive::{Serialize, Deserialize};
use gst::prelude::*;
use gst_app::{AppSink};
use log::{error, info, warn, debug};
use std::env;
use std::ffi::{CString};
use std::thread::sleep;

trait ToCString {
    fn to_c_string(&self) -> Result<CString, std::ffi::NulError>;
}
impl ToCString for &str {
    fn to_c_string(&self) -> Result<CString, std::ffi::NulError> {
        CString::new(&**self)
    }
}

impl ToCString for String {
    fn to_c_string(&self) -> Result<CString, std::ffi::NulError> {
        self.as_str().to_c_string()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppConfig {
    cert_path: String,
    app_id: String,
    channel_name: String,
    app_token: String,
    log_path: String,
    uid: u32
}

impl Default for AppConfig {
    fn default() -> AppConfig {
        let home = dirs::home_dir().unwrap();
        AppConfig {
            cert_path: home.join("certificate.bin").to_str().unwrap().into(),
            app_id: "".into(),
            channel_name: "test".into(),
            app_token: "".into(),
            log_path: "logs".into(),
            uid: 1234,
        }
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

fn main() -> Result<(), anyhow::Error> {
    // Set the default log level to debug
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    // Output the log to STDOUT
    let mut builder = env_logger::Builder::from_default_env();
    builder.target(env_logger::Target::Stdout);
    builder.init();

    let app_name = "agora-rtsa";
    let res: Result<AppConfig, _> = confy::load(app_name, None);
    let path = confy::get_configuration_file_path(&app_name, None).unwrap();
    info!("Reading config file from {:#?}", path);
    let cfg:AppConfig = match res {
        Ok(cfg) => cfg,
        Err(e) => {
            error!(
                "Parse Config Error: Please check your configuration file at {:#?}",
                path
            );
            panic!("{}", e)
        }
    };
    dbg!(&cfg);
    let cert = std::fs::read_to_string(cfg.cert_path)?;
    let app_id = cfg.app_id;
    let channel_name = cfg.channel_name;
    let app_token = cfg.app_token;
    let log_path = cfg.log_path;
    let uid: u32 = 1234;

    gst::init().unwrap();

    info!("Agora Version: {}", agoraRTC::get_version());
    result_verify(agoraRTC::license_verify(cert.as_str()), "license_verify");

    let handlers = C::agora_rtc_event_handler_t::new();
    let log_cfg = agoraRTC::LogConfig {
        log_disable: false,
        log_disable_desensitize: true,
        log_level: LogLevel::INFO,
    };

    let opt = agoraRTC::RtcServiceOption {
        area_code: agoraRTC::AreaCode::CN,
        product_id: [0; 64],
        log_cfg: log_cfg,
        license_value: [0; 33],
    };

    // You have to set logs before deallocation
    let log_path_c = log_path.to_c_string().unwrap();
    let opt_t: C::rtc_service_option_t = opt.to_c_type(&log_path_c);

    /// If frame_per_sec equals zero, then real timestamp will be used. So I don't need to calculate them.
    let video_opt = C::video_frame_info_t {
        data_type: VideoDataType::H264.into(),
        stream_type: VideoStreamQuality::LOW.into(),
        frame_type: VideoFrameType::AUTO.into(),
        frame_rate: 0,
    };

    // Create the GStreamer pipeline
    let pipeline = gst::parse_launch(
        "v4l2src device=/dev/video9 ! \
        clockoverlay ! \
        videoconvert ! \
        videoscale ! \
        video/x-raw,width=320,height=240 ! \
        x264enc speed-preset=ultrafast tune=zerolatency ! \
        queue ! \
        appsink name=agora ",
    )
    .expect("not a elem");

    // Downcast from gst::Element to gst::Pipeline
    let pipeline = pipeline
        .downcast::<gst::Pipeline>()
        .expect("not a pipeline");
    // https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/-/blob/main/tutorials/src/bin/basic-tutorial-8.rs
    let appsink = pipeline
        .by_name("agora")
        .expect("can't find agora")
        .dynamic_cast::<AppSink>()
        .expect("should be an appsink");

    result_verify(agoraRTC::init(&app_id.to_c_string().unwrap(), opt_t, handlers), "init");
    let conn_id: u32 = match agoraRTC::create_connection() {
        Ok(id) => id,
        Err(code) => {
            let reason = agoraRTC::err_2_reason(code);
            panic!("create_connection failed. Reason: {}", reason);
        }
    };

    let mut file = std::fs::File::create("buf.out.h264").unwrap();
    appsink.clone().set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |_| {
                if let Ok(sample) = appsink.pull_sample() {
                    // See also gst::Buffer.copy_to_slice
                    // https://docs.rs/gstreamer/0.18.8/gstreamer/buffer/struct.Buffer.html#method.copy_to_slice
                    let buf = sample.buffer().unwrap().copy();
                    let mem = buf.all_memory().unwrap();
                    let readable = mem.into_mapped_memory_readable().unwrap();
                    let slice = readable.as_slice(); // this shit is the actual buffer
                    // the buffer contains a media specific marker. for video this is the end of a frame boundary, for audio this is the start of a talkspurt.
                    // https://gstreamer.freedesktop.org/documentation/gstreamer/gstbuffer.html?gi-language=c#GstBufferFlags
                    let flags = buf.flags().contains(gst::BufferFlags::MARKER);
                    if flags {
                        agoraRTC::send_video_data(conn_id, slice, &video_opt).unwrap();
                        file.write(slice).unwrap();
                        // print a star to stdout
                        use std::io::{self, Write};
                        print!("*");
                        let _ = io::stdout().flush();
                    }
                }

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    let chan_opt = C::rtc_channel_options_t::new();
    result_verify(
        agoraRTC::join_channel(
            conn_id,
            &channel_name.to_c_string().unwrap(),
            Some(uid),
            &app_token.to_c_string().unwrap(),
            chan_opt,
        ),
        "join channel",
    );

    result_verify(agoraRTC::mute_local_audio(conn_id, true), "mute local audio");

    // add some delay to make sure header sent
    sleep(std::time::Duration::from_secs(2));

    pipeline
        .set_state(gst::State::Playing)
        .expect("set playing error");

    sleep(std::time::Duration::from_secs(120));

    pipeline
        .set_state(gst::State::Null)
        .expect("set state error");
    result_verify(agoraRTC::leave_channel(conn_id), "leave channel");
    result_verify(agoraRTC::destroy_connection(conn_id), "destory connection");
    result_verify(agoraRTC::deinit(), "dinit");
    Ok(())
}
