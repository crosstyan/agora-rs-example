extern crate dirs;
use agora_rtsa_rs::agoraRTC;
use agora_rtsa_rs::agoraRTC::AgoraApp;
use agora_rtsa_rs::agoraRTC::{LogLevel, VideoDataType, VideoFrameType, VideoStreamQuality};
use agora_rtsa_rs::C::{self};
use anyhow::bail;
use gst::prelude::*;
use gst_app::AppSink;
use log::{debug, error, info, warn};
use serde_derive::{Deserialize, Serialize};
use std::env;
use std::thread::sleep;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppConfig {
    cert_path: String,
    app_id: String,
    channel_name: String,
    app_token: String,
    log_path: String,
    uid: u32,
    out_file_path: String,
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
            out_file_path: "".into(),
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
    let cfg: AppConfig = match res {
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

    // Create the GStreamer pipeline
    let pipeline = gst::parse_launch(
        "videotestsrc name=src is-live=true  ! \
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

    info!("Agora Version: {}", agoraRTC::get_version());
    result_verify(
        agoraRTC::AgoraApp::license_verify(cert.as_str()),
        "license_verify",
    );
    let mut app = AgoraApp::new(&app_id);
    let service_opt = agoraRTC::RtcServiceOption::new(&log_path, LogLevel::DEBUG);

    result_verify(app.init(service_opt), "init");
    let res = app.create_connection();
    match res {
        Ok(conn_id) => info!("connection id is {}", conn_id),
        Err(e) => error!(
            "can't create connection. reason: {}",
            agoraRTC::err_2_reason(e)
        ),
    }

    // If frame_per_sec equals zero, then real timestamp will be used. So I don't need to calculate them.
    let video_info = C::video_frame_info_t {
        data_type: VideoDataType::H264.into(),
        stream_type: VideoStreamQuality::LOW.into(),
        frame_type: VideoFrameType::AUTO.into(),
        frame_rate: 0,
    };
    app.set_video_info(video_info);

    // if out_file_path is not empty create such file to write
    let mut maybe_file = match cfg.out_file_path.as_str() {
        "" => None,
        _ => Some(std::fs::File::create(cfg.out_file_path)?),
    };

    let chan_opt = C::rtc_channel_options_t::new();
    result_verify(
        app.join_channel(&channel_name, Some(uid), &app_token, chan_opt),
        "join channel",
    );

    result_verify(app.mute_local_audio(true), "mute local audio");

    // app is moved now and you can't use it anymore
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
                        use std::io::{self, Write};
                        let code = app.send_video_data_default(slice);
                        if let Some(file) = &mut maybe_file {
                            file.write(slice).unwrap();
                        }
                        match code {
                            Ok(_) => {
                                // print a star to stdout
                                print!("*");
                                let _ = io::stdout().flush();
                            },
                            Err(_e) => {
                                print!("x");
                                let _ = io::stdout().flush();
                            },
                        }
                    }
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    // add some delay to make sure agora is ready
    sleep(std::time::Duration::from_secs(2));

    pipeline
        .set_state(gst::State::Playing)
        .expect("set playing error");

    sleep(std::time::Duration::from_secs(120));

    pipeline
        .set_state(gst::State::Null)
        .expect("set state error");
    // AgoraApp should be dropped here
    Ok(())
}
