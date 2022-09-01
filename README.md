```rust
let pipe = "videotestsrc name=src is-live=true ! \
    clockoverlay ! \
    videoconvert ! \
    x264enc ! \
    appsink name=agora "

let pipeline = gst::parse_launch(
    "v4l2src device=/dev/video9 ! \
    clockoverlay ! \
    videoconvert ! \
    videoscale ! \
    video/x-raw,width=320,height=240 ! \
    x264enc speed-preset=ultrafast tune=zerolatency ! \
    queue ! \
    appsink name=agora ",
).expect("not a elem");
```