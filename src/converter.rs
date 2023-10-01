use std::{cell::RefCell, time::Duration};

use crate::time_parsing::{progress_from_durations, try_extract_duration, try_extract_frame_time};

use super::{Command, Error, Message, Settings};

#[cfg(not(feature = "tokio"))]
/// The sender's end of an mpsc [`Command`] channel.
pub type CommandSender = std::sync::mpsc::Sender<Command>;
#[cfg(not(feature = "tokio"))]
/// The reciever's end of an mpsc [`Command`] channel.
pub type CommandReceiver = std::sync::mpsc::Receiver<Command>;
#[cfg(not(feature = "tokio"))]
/// The sender's end of an mpsc [`Message`] channel.
pub type MessageReceiver = std::sync::mpsc::Receiver<Message>;
#[cfg(not(feature = "tokio"))]
/// The reciever's end of an mpsc [`Message`] channel.
pub type MessageSender = std::sync::mpsc::Sender<Message>;

#[cfg(feature = "tokio")]
/// The sender's end of an mpsc [`Command`] channel.
pub type CommandSender = tokio::sync::mpsc::UnboundedSender<Command>;
#[cfg(feature = "tokio")]
/// The reciever's end of an mpsc [`Command`] channel.
pub type CommandReceiver = tokio::sync::mpsc::UnboundedReceiver<Command>;
#[cfg(feature = "tokio")]
/// The sender's end of an mpsc [`Message`] channel.
pub type MessageReceiver = tokio::sync::mpsc::UnboundedReceiver<Message>;
#[cfg(feature = "tokio")]
/// The reciever's end of an mpsc [`Message`] channel.
pub type MessageSender = tokio::sync::mpsc::UnboundedSender<Message>;

/// A structure containing the information required to start
/// and perform the conversion job.
pub struct Converter {
    /// The sender's end of the [`Message`] channel.
    tx: MessageSender,
    /// The receiver's end of the [`Command`] channel, wrapped inside
    /// an [`Option`] and then again inside a [`std::cell::RefCell`].
    rx: RefCell<Option<CommandReceiver>>,
    /// Whether the job was cancelled.
    ///
    /// NOTE: Technically, this wouldn't have to be stored in the structure,
    /// but it's OK for now.
    job_cancelled: std::sync::Arc<std::sync::Mutex<bool>>,
}

impl Converter {
    /// A factory method that takes care of creating the channels to send [`Message`]'s
    /// and [`Command`]'s between the [`Converter`] and the application. The method returns
    /// a tuple containing the [`Converter`], the [`CommandSender`], and the [`MessageReceiver`],
    pub fn new_with_channels() -> (Self, CommandSender, MessageReceiver) {
        #[cfg(not(feature = "tokio"))]
        let (command_tx, command_rx): (CommandSender, CommandReceiver) = std::sync::mpsc::channel();
        #[cfg(not(feature = "tokio"))]
        let (message_tx, message_rx): (MessageSender, MessageReceiver) = std::sync::mpsc::channel();

        #[cfg(feature = "tokio")]
        let (command_tx, command_rx): (CommandSender, CommandReceiver) =
            tokio::sync::mpsc::unbounded_channel();
        #[cfg(feature = "tokio")]
        let (message_tx, message_rx): (MessageSender, MessageReceiver) =
            tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                tx: message_tx,
                rx: RefCell::new(Some(command_rx)),
                job_cancelled: std::sync::Arc::new(std::sync::Mutex::new(false)),
            },
            command_tx,
            message_rx,
        )
    }

    pub fn convert(self, settings: Settings) {
        let mut child = std::process::Command::new("ffmpeg")
            .arg("-stats")
            .arg("-i")
            .arg(&settings.video_path)
            .arg("-filter_complex")
            .arg(settings.generate_filter_complex())
            .arg("-f")
            .arg("gif")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn child process");

        let mut stdin = child
            .stdin
            .take()
            .expect("Failed to take stdin from child process");
        let mut stdout = child
            .stdout
            .take()
            .expect("Failed to take stdout from child process");
        let mut stderr = child
            .stderr
            .take()
            .expect("Failed to take stderr from child process");

        let tx_stdin = self.tx.clone();
        #[cfg(not(feature = "tokio"))]
        let rx_command = self.rx.take().expect("Unable to take command receiver");
        #[cfg(feature = "tokio")]
        let mut rx_command = self.rx.take().expect("Unable to take command receiver");
        let job_cancelled_stdin = std::sync::Arc::clone(&self.job_cancelled);
        let handle_stdin = std::thread::spawn(move || {
            use std::io::Write;
            loop {
                #[cfg(not(feature = "tokio"))]
                let recv = rx_command.recv().ok();
                #[cfg(feature = "tokio")]
                let recv = rx_command.blocking_recv();

                match recv {
                    Some(c) => match c {
                        Command::Cancel => {
                            stdin.write_all(b"q").expect("Failed to write on STDIN");
                            tx_stdin
                                .send(Message::Error(Error::Cancelled))
                                .expect("Failed to send 'cancelled' message");
                            {
                                let mut job_cancelled = job_cancelled_stdin.lock().unwrap();
                                *job_cancelled = true;
                            }
                            break;
                        }
                    },
                    None => {
                        break;
                    }
                }
            }
        });

        let tx_stdout = self.tx.clone();
        let job_cancelled_stdout = std::sync::Arc::clone(&self.job_cancelled);
        let handle_stdout = std::thread::spawn(move || {
            use std::io::Read;

            let mut buf: Vec<u8> = vec![];
            match stdout.read_to_end(&mut buf) {
                Err(e) => panic!("Failed to read to end: {:?}", e),
                Ok(_) => {
                    let job_cancelled = job_cancelled_stdout.lock().unwrap();
                    if !*job_cancelled {
                        tx_stdout
                            .send(Message::Success(buf))
                            .expect("Failed to send 'data'");
                    }
                }
            }
        });

        let tx_stderr = self.tx.clone();
        let handle_stderr = std::thread::spawn(move || {
            use std::io::Read;

            let mut duration: Option<Duration> = None;

            let mut full_buffer: Vec<u8> = vec![];
            let mut buffer = vec![0u8; 1000]; // this needs to be set such that we'll be able to get "Duration unbroken" (frame should be ok)
            loop {
                match stderr.read(&mut buffer) {
                    Ok(n) => {
                        if n > 0 {
                            full_buffer.append(&mut buffer[..n].to_vec());

                            if duration.is_none() {
                                let s = std::str::from_utf8(&full_buffer[..])
                                    .expect("Failed to parse buffer into string");
                                if let Some(d) = try_extract_duration(s) {
                                    duration = Some(d);
                                    tx_stderr
                                        .send(Message::VideoDuration(d))
                                        .expect("Failed to send video duration message");
                                }
                            }

                            let s = std::str::from_utf8(&buffer[..n])
                                .expect("Failed to parse bytes as string");

                            if s.starts_with("frame=") {
                                if let Some(time) = try_extract_frame_time(s) {
                                    if let Some(duration) = duration {
                                        tx_stderr
                                            .send(Message::Progress(progress_from_durations(
                                                duration, time,
                                            )))
                                            .expect("Failed to send progress message");
                                    }
                                } else {
                                    panic!("NOTE: frame= received without duration parsed. Please fix this.");
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::WouldBlock => {}
                        _ => panic!("{:?}", e),
                    },
                }
            }
        });

        let tx_child = self.tx.clone();
        let handle_child = std::thread::spawn(move || match child.wait() {
            Ok(status) => {
                if let Some(code) = status.code() {
                    if code > 0 {
                        tx_child
                            .send(Message::Error(Error::ExitCode(code)))
                            .expect("Failed to send exit status error");
                    }
                }
            }
            Err(e) => tx_child
                .send(Message::Error(Error::ChildProcess(std::sync::Arc::new(e))))
                .expect("Failed to send exit unknown error"),
        });

        handle_child
            .join()
            .expect("Failed to join the 'child process' thread");
        handle_stderr
            .join()
            .expect("Failed to join the 'stderr' thread");
        handle_stdout
            .join()
            .expect("Failed to join the 'stdout' thread");
        handle_stdin
            .join()
            .expect("Failed to join the 'stdin' thread");
    }
}
