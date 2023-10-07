use ffmpeg_gif_maker::{Converter, Message, Settings};

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
            Message::Done => {
                println!("Done message received, so breaking loop...");
                break;
            }
            Message::Error(e) => {
                eprintln!("Error message recived: {:?}", e);
            }
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
