#![doc = include_str!("../docs/lib.md")]

pub use converter::{CommandReceiver, CommandSender, Converter, MessageReceiver, MessageSender};

mod converter;
mod time_parsing;

#[derive(Clone, Debug)]
/// The structure that contains the settings for the [`Converter`].
pub struct Settings {
    /// The absolute path of the FFmpeg binary on the system.
    ffmpeg_path: Option<String>,
    /// The path of the video to be converted into an animated GIF.
    video_path: String,
    /// The frame rate (in frames per second) to use for animated GIF.
    gif_fps: u16,
    /// The animated GIF's width.
    gif_width: u16,
}

impl Settings {
    /// The default frame rate used for the generated animated GIF.
    ///
    /// NOTE: This is the only allowed value for now; i.e. the API does
    /// not allow modifying this value.
    pub const STANDARD_FPS: u16 = 10;

    /// A factory method that takes in the source `video_path` and the
    /// target `width` for the animated GIF.
    pub fn with_standard_fps(video_path: String, width: u16) -> Self {
        Self {
            ffmpeg_path: None,
            video_path,
            gif_fps: Self::STANDARD_FPS,
            gif_width: width,
        }
    }

    /// A setter method that allows specifying the path to be used
    /// for the ffmpeg binary.
    pub fn ffmpeg_path(self, ffmpeg_path: impl Into<String>) -> Self {
        Self {
            ffmpeg_path: Some(ffmpeg_path.into()),
            ..self
        }
    }

    /// A convenience method that can be used to generate the
    /// value of FFmpeg's `-filter_complex` flag.
    fn generate_filter_complex(&self) -> String {
        format!(
            "fps={},scale={}:-1[s]; [s]split[a][b]; [a]palettegen[palette]; [b][palette]paletteuse",
            self.gif_fps, self.gif_width
        )
    }
}

#[derive(Debug, Clone)]
/// An error generated by the [`Converter`].
pub enum Error {
    /// Contains an error code returned by the FFmpeg child process.
    /// 
    /// NOTE: I am not sure at this point whether this variant will ever get emitted.
    /// For instance, deliberately inputting an invalid file format (e.g. png)
    /// will still return 0 (i.e. success) as an exit code, but we will simply
    /// get an empty `stdout`, which we signal here using the [`Error::EmptyStdout`]
    /// variant instead.
    ExitCode(i32),
    /// A confirmation that signals that the conversion job has been cancelled.
    Cancelled,
    /// Contains the [`std::io::Error`] returned by calling the `wait` method
    /// on the [`std::process::Child`] process.
    ChildProcess(std::sync::Arc<std::io::Error>),
    /// Emitted by the [`Converter`] when the child process' `stdout` is
    /// empty at the end of the job. This is likely because an invalid file
    /// was input. Since this library is currently not parsing FFmpeg's logs
    /// for error messages, we simply assume that an empty `stdout` means an
    /// unsupported file format.
    EmptyStdout,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone)]
/// A message (i.e. an event) sent to the application by the [`Converter`].
pub enum Message {
    /// The raw bytes that make up the successfully generated animated GIF.
    Success(Vec<u8>),
    /// An error message, containing the [`Error`].
    Error(Error),
    /// The progress (a value between 0.0 and 1.0) made by the converter, estimated
    /// by taking the number of processed frames divided by the total number
    /// of frames.
    ///
    /// NOTE: Progress messages don't start being emitted right away.
    /// The [`Message::VideoDuration`] will (should) be emitted first.
    Progress(f64),
    /// The video duration, determined by FFmpeg as a first step in creating
    /// the animated GIF. Note that this event will (should) be emitted before
    /// the [`Message::Progress`] event.
    VideoDuration(std::time::Duration),
    /// A message that signals that the job is done and that no other messages
    /// will be emitted.
    Done,
}

#[derive(Debug, Clone)]
/// A command sent to the [`Converter`] by the application.
pub enum Command {
    /// A request to terminate the conversion job. If successful,
    /// this command will result in an [`Error::Cancelled`] emitted
    /// as a [`Message::Error`].
    Cancel,
}
