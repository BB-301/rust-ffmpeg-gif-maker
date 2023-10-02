# Animated GIF Generator Rust Library Based On A Simple FFmpeg Child Process Wrapper

This is a simple, very experimental Rust library that makes a system call to `FFmpeg` to generate an animated GIF from a video path.

## Disclaimer

This project is still (and will likely always remain) in an early experimental state and was not thoroughly tested. I created this project as part of another toy project (_add link here_) and stopped as soon as I had a working prototype on my development system (i.e. macOS).

## Requirements

* The library assumes that the system has `ffmpeg version 5.0-tessus` installed on its path. You may run `ffmpeg -version` in a terminal to confirm that. See [FFmpeg - Downloads](https://ffmpeg.org/download.html#releases) if you need to install it. It is possible that the library will work with other versions, but this was not tested.

## Example

```
use ffmpeg_gif_maker::{Converter, Error, Message, Settings};

const INPUT_VIDEO_PATH: &'static str = "./assets/big-buck-bunny-clip.mp4";
const OUTPUT_GIF_WIDTH: u16 = 200;

#[tokio::main]
async fn main() {
    let settings = Settings::with_standard_fps(INPUT_VIDEO_PATH.into(), OUTPUT_GIF_WIDTH);

    let (converter, _, mut rx) = Converter::new_with_channels();

    let handle_converter_task = tokio::task::spawn_blocking(move || {
        converter.convert(settings);
    });

    loop {
        match rx.recv().await.expect("Other end of channel was closed?") {
            Message::Error(e) => match e {
                Error::Cancelled => {
                    // NOTE: This will never get called here because we don't perform
                    // any cancellation command.
                    println!("Received cancellation confirmation, so leaving...");
                    break;
                }
                _ => {
                    panic!("Received and error: {:?}", e);
                }
            },
            Message::Success(bytes) => {
                // NOTE: You could save the output to a file here.
                println!("Generated GIF size: {} bytes", bytes.len());
                break;
            }
            Message::Progress(progress) => {
                println!("Progress: {:.02} %", (progress * 100.0).round() / 100.0);
            }
            Message::VideoDuration(duration) => {
                println!("Received info about video duration: {:?}", duration);
            }
        }
    }

    println!("Waiting for converter thread to exit...");
    handle_converter_task.await.expect("Failed to join");
    println!("All done!");
}
```

See the crate's [repository](https://github.com/BB-301/rust-ffmpeg-gif-maker) for more examples and details about the project.