use std::{cell::RefCell, time::Duration};

use crate::time_parsing::{progress_from_durations, try_extract_duration, try_extract_frame_time};

use super::{Command, Error, Message, Settings};

const STDIN_THREAD_SLEEP_DURATION_MS: u64 = 50;

const LOG_TARGET_MAIN: &'static str = "ffmpeg_gif_maker::converter::main_thread";
const LOG_TARGET_STDIN: &'static str = "ffmpeg_gif_maker::converter::stdin_thread";
const LOG_TARGET_STDOUT: &'static str = "ffmpeg_gif_maker::converter::stdout_thread";
const LOG_TARGET_STDERR: &'static str = "ffmpeg_gif_maker::converter::stderr_thread";
const LOG_TARGET_CHILD: &'static str = "ffmpeg_gif_maker::converter::child_thread";

#[cfg(not(feature = "tokio"))]
/// The sender's end of an mpsc [`Command`] channel.
pub type CommandSender = std::sync::mpsc::Sender<Command>;
#[cfg(not(feature = "tokio"))]
/// The reciever's end of an mpsc [`Command`] channel.
pub type CommandReceiver = std::sync::mpsc::Receiver<Command>;
#[cfg(not(feature = "tokio"))]
/// The sender's end of an mpsc [`Message`] channel.
pub type MessageSender = std::sync::mpsc::Sender<Message>;
#[cfg(not(feature = "tokio"))]
/// The reciever's end of an mpsc [`Message`] channel.
pub type MessageReceiver = std::sync::mpsc::Receiver<Message>;

#[cfg(feature = "tokio")]
/// The sender's end of an mpsc [`Command`] channel.
pub type CommandSender = tokio::sync::mpsc::UnboundedSender<Command>;
#[cfg(feature = "tokio")]
/// The reciever's end of an mpsc [`Command`] channel.
pub type CommandReceiver = tokio::sync::mpsc::UnboundedReceiver<Command>;
#[cfg(feature = "tokio")]
/// The sender's end of an mpsc [`Message`] channel.
pub type MessageSender = tokio::sync::mpsc::UnboundedSender<Message>;
#[cfg(feature = "tokio")]
/// The reciever's end of an mpsc [`Message`] channel.
pub type MessageReceiver = tokio::sync::mpsc::UnboundedReceiver<Message>;

/// A structure containing the information required to
/// perform the conversion job.
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
    /// Whether the job has ended (i.e. the child process's `stdout` has returned).
    ///
    /// NOTE: Just like `job_cancelled`, this wouldn't have to be stored in the structure,
    /// but it's OK for now (besides, better be consistent).
    job_ended: std::sync::Arc<std::sync::Mutex<bool>>,
    /// A unique identifier for the instance, used by internal logging logic
    /// to be able to output meaningful logs.
    id: uuid::Uuid,
}

impl Converter {
    /// A unique identifier for the instance, used by internal logging logic
    /// to be able to output meaningful logs.
    pub fn id(&self) -> uuid::Uuid {
        self.id
    }

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
        let out = (
            Self {
                tx: message_tx,
                rx: RefCell::new(Some(command_rx)),
                job_cancelled: std::sync::Arc::new(std::sync::Mutex::new(false)),
                job_ended: std::sync::Arc::new(std::sync::Mutex::new(false)),
                id: uuid::Uuid::new_v4(),
            },
            command_tx,
            message_rx,
        );
        log::info!(target: LOG_TARGET_MAIN, "{} Instance created", out.0.id());
        out
    }

    pub fn convert(self, settings: Settings) {
        log::debug!(target: LOG_TARGET_MAIN, "{} Trying to spawn FFmpeg child process...", self.id());
        let binary_path = match &settings.ffmpeg_path {
            Some(path) => {
                log::info!(target: LOG_TARGET_MAIN, "{} FFmpeg binary path provided: {}", self.id(), path);
                path.to_string()
            }
            None => {
                log::info!(target: LOG_TARGET_MAIN, "{} No FFmpeg binary path provided, so expecting to find 'ffmpeg' on system path.", self.id());
                "ffmpeg".to_string()
            }
        };
        let mut child = match std::process::Command::new(binary_path)
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
        {
            Ok(c) => {
                log::debug!(target: LOG_TARGET_MAIN, "{} FFmpeg child process successfully spawned.", self.id());
                c
            }
            Err(e) => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to spawn child process: {:?}", self.id(), e);
                panic!();
            }
        };

        let mut stdin = match child.stdin.take() {
            Some(io) => io,
            None => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to take STDIN from child process.", self.id());
                panic!();
            }
        };
        let mut stdout = match child.stdout.take() {
            Some(io) => io,
            None => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to take STDOUT from child process.", self.id());
                panic!();
            }
        };
        let mut stderr = match child.stderr.take() {
            Some(io) => io,
            None => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to take STDERR from child process.", self.id());
                panic!()
            }
        };

        let tx_stdin = self.tx.clone();
        #[cfg(not(feature = "tokio"))]
        let Some(rx_command) = self.rx.take() else {
            log::error!(target: LOG_TARGET_MAIN, "{} Unable to take command receiver.", self.id());
            panic!();
        };
        #[cfg(feature = "tokio")]
        let Some(mut rx_command) = self.rx.take() else {
            log::error!(target: LOG_TARGET_MAIN, "{} Unable to take command receiver.", self.id());
            panic!();
        };
        let job_cancelled_stdin = std::sync::Arc::clone(&self.job_cancelled);
        let job_ended_stdin = std::sync::Arc::clone(&self.job_ended);
        let id_stdin = self.id();
        let handle_stdin = std::thread::spawn(move || {
            log::info!(target: LOG_TARGET_STDIN, "{} Entered STDIN thread.", id_stdin);
            {
                use std::io::Write;
                // NOTE: Here (i.e. inside the loop) we use `trace` instead of `debug` because we are no longer
                // "receive blocking": we are no polling the channel. The reason for polling instead of blocking is that
                // we needed a way for this thread to check whether the child process' stdout
                // had returned, else the current thread would keep waiting until receiving
                // a "Cancel" command or the other channel's end being dropped.
                loop {
                    #[cfg(not(feature = "tokio"))]
                    let recv = rx_command.try_recv();
                    #[cfg(feature = "tokio")]
                    let recv = rx_command.try_recv();

                    log::trace!(target: LOG_TARGET_STDIN, "{} Non-blockingly polling channel for next message...", id_stdin);
                    match recv {
                        Ok(c) => match c {
                            Command::Cancel => {
                                log::info!(target: LOG_TARGET_STDIN, "{} Received 'cancel' command.", id_stdin);
                                log::trace!(target: LOG_TARGET_STDIN, "{} Trying to write 'q' to STDIN...", id_stdin);
                                match stdin.write_all(b"q") {
                                    Ok(_) => {
                                        log::trace!(target: LOG_TARGET_STDIN, "{} Successfully wrote 'q' to STDIN.", id_stdin);
                                    }
                                    Err(e) => {
                                        log::error!(target: LOG_TARGET_STDIN, "{} Failed to write 'q' to STDIN: {:?}", id_stdin, e);
                                        panic!();
                                    }
                                }
                                log::trace!(target: LOG_TARGET_STDIN, "{} Trying to send cancellation confirmation message...", id_stdin);
                                match tx_stdin.send(Message::Error(Error::Cancelled)) {
                                    Ok(_) => {
                                        log::trace!(target: LOG_TARGET_STDIN, "{} Successfully sent cancellation confirmation message.", id_stdin);
                                    }
                                    Err(e) => {
                                        log::error!(target: LOG_TARGET_STDIN, "{} Failed to send cancellation confirmation message: {:?}", id_stdin, e);
                                        panic!();
                                    }
                                }
                                {
                                    log::trace!(target: LOG_TARGET_STDIN, "{} Trying to acquire job cancellation mutex to set it to 'true'...", id_stdin);
                                    let mut job_cancelled = match job_cancelled_stdin.lock() {
                                        Ok(m) => {
                                            log::trace!(target: LOG_TARGET_STDIN, "{} Job cancellation mutex successfully acquired and set 'true'.", id_stdin);
                                            m
                                        }
                                        Err(e) => {
                                            log::error!(target: LOG_TARGET_STDIN, "{} Failed to acquire job cancellation mutex: {:?}", id_stdin, e);
                                            panic!();
                                        }
                                    };
                                    *job_cancelled = true;
                                }
                                log::info!(target: LOG_TARGET_STDIN, "{} Breaking out of STDIN thread because job cancelled...", id_stdin);
                                break;
                            }
                        },
                        #[cfg(feature = "tokio")]
                        Err(e) => match e {
                            tokio::sync::mpsc::error::TryRecvError::Empty => {
                                log::trace!(target: LOG_TARGET_STDIN, "{} Channel empty. Sleeping for {} milliseconds...", id_stdin, STDIN_THREAD_SLEEP_DURATION_MS);
                                std::thread::sleep(std::time::Duration::from_millis(
                                    STDIN_THREAD_SLEEP_DURATION_MS,
                                ));
                            }
                            tokio::sync::mpsc::error::TryRecvError::Disconnected => {
                                log::info!(target: LOG_TARGET_STDIN, "{} Breaking out of STDIN thread because channel closed...", id_stdin);
                                break;
                            }
                        },
                        #[cfg(not(feature = "tokio"))]
                        Err(e) => match e {
                            std::sync::mpsc::TryRecvError::Empty => {
                                log::trace!(target: LOG_TARGET_STDIN, "{} Channel empty. Sleeping for {} milliseconds...", id_stdin, STDIN_THREAD_SLEEP_DURATION_MS);
                                std::thread::sleep(std::time::Duration::from_millis(
                                    STDIN_THREAD_SLEEP_DURATION_MS,
                                ));
                            }
                            std::sync::mpsc::TryRecvError::Disconnected => {
                                log::info!(target: LOG_TARGET_STDIN, "{} Breaking out of STDIN thread because channel closed...", id_stdin);
                                break;
                            }
                        },
                    }

                    log::trace!(target: LOG_TARGET_STDIN, "{} Trying to acquire 'job ended' mutex to see if the job has completed...", id_stdin);
                    let job_ended = match job_ended_stdin.lock() {
                        Err(e) => {
                            log::error!(target: LOG_TARGET_STDIN, "{} Failed to acquire 'job ended' mutex: {:?}", id_stdin, e);
                            panic!();
                        }
                        Ok(m) => {
                            log::trace!(target: LOG_TARGET_STDIN, "{} Successfully acquired 'job ended' mutex.", id_stdin);
                            m
                        }
                    };
                    if *job_ended {
                        log::info!(target: LOG_TARGET_STDIN, "{} Job has ended, so breaking out of 'read loop'...", id_stdin);
                        break;
                    } else {
                        log::trace!(target: LOG_TARGET_STDIN, "{} Job has not ended yet.", id_stdin);
                    }
                }

                log::info!(target: LOG_TARGET_STDIN, "{} Exiting STDIN thread...", id_stdin);
            }
        });

        let tx_stdout = self.tx.clone();
        let job_cancelled_stdout = std::sync::Arc::clone(&self.job_cancelled);
        let job_ended_stdout = std::sync::Arc::clone(&self.job_ended);
        let id_stdout = self.id();
        let handle_stdout = std::thread::spawn(move || {
            log::info!(target: LOG_TARGET_STDOUT, "{} Entered STDOUT thread.", id_stdout);

            use std::io::Read;

            let mut buf: Vec<u8> = vec![];
            log::info!(target: LOG_TARGET_STDOUT, "{} Waiting to read all STDOUT bytes into buffer...", id_stdout);
            match stdout.read_to_end(&mut buf) {
                Err(e) => {
                    log::error!(target: LOG_TARGET_STDOUT, "{} Failed to read to end: {:?}", id_stdout, e);
                    panic!();
                }
                Ok(n) => {
                    log::info!(target: LOG_TARGET_STDOUT, "{} Successfully read to end (size: {}).", id_stdout, n);
                    log::trace!(target: LOG_TARGET_STDOUT, "{} Logging full buffer:\n{:?}", id_stdout, buf);

                    log::debug!(target: LOG_TARGET_STDOUT, "{} Trying to acquire job cancellation mutex to check whether job has been cancelled, to avoid sending bytes down channel it case it has...", id_stdout);
                    let job_cancelled = {
                        let job_cancelled = match job_cancelled_stdout.lock() {
                            Ok(m) => {
                                log::debug!(target: LOG_TARGET_STDOUT, "{} Successfully acquired job cancellation mutex.", id_stdout);
                                m
                            }
                            Err(e) => {
                                log::error!(target: LOG_TARGET_STDOUT, "{} Failed to acquire job cancellation mutex: {:?}", id_stdout, e);
                                panic!();
                            }
                        };
                        *job_cancelled
                    };

                    if !job_cancelled {
                        log::debug!(target: LOG_TARGET_STDOUT, "{} Job has not been cancelled, so checking whether there is data in buffer...", id_stdout);
                        if buf.is_empty() {
                            log::warn!(target: LOG_TARGET_STDOUT, "{} Empty buffer found, so send 'empty stdout' error message down channel.", id_stdout);
                            match tx_stdout.send(Message::Error(Error::EmptyStdout)) {
                                Ok(_) => {
                                    log::debug!(target: LOG_TARGET_STDOUT, "{} Successfully sent error message down channel.", id_stdout);
                                }
                                Err(e) => {
                                    log::error!(target: LOG_TARGET_STDOUT, "{} Failed to send error message down channel: {:?}", id_stdout, e);
                                    panic!();
                                }
                            }
                        } else {
                            match tx_stdout.send(Message::Success(buf)) {
                                Ok(_) => {
                                    log::debug!(target: LOG_TARGET_STDOUT, "{} Successfully sent STDOUT data down channel.", id_stdout);
                                }
                                Err(e) => {
                                    log::error!(target: LOG_TARGET_STDOUT, "{} Failed to send STDOUT data down channel: {:?}", id_stdout, e);
                                    panic!();
                                }
                            }
                        }
                    } else {
                        log::warn!(target: LOG_TARGET_STDOUT, "{} Job has been marked as cancelled, so not sending data down channel.", id_stdout);
                    }
                }
            }

            log::debug!(target: LOG_TARGET_STDOUT, "{} Trying to acquire 'job ended' mutex to set it to 'true'...", id_stdout);
            let mut job_ended = match job_ended_stdout.lock() {
                Err(e) => {
                    log::error!(target: LOG_TARGET_STDOUT, "{} Failed to acquire 'job ended' mutex to set it to 'true': {:?}", id_stdout, e);
                    panic!();
                }
                Ok(m) => {
                    log::debug!(target: LOG_TARGET_STDOUT, "{} Successfully acquired 'job ended' mutex and set it to 'true'.", id_stdout);
                    m
                }
            };
            *job_ended = true;

            log::info!(target: LOG_TARGET_STDOUT, "{} Exiting STDOUT thread...", id_stdout);
        });

        let tx_stderr = self.tx.clone();
        let id_stderr = self.id();
        let job_cancelled_stderr = std::sync::Arc::clone(&self.job_cancelled);
        let handle_stderr = std::thread::spawn(move || {
            log::info!(target: LOG_TARGET_STDERR, "{} Entered STDERR thread.", id_stderr);

            use std::io::Read;

            let id_stderr_string = id_stderr.to_string();
            let mut duration: Option<Duration> = None;

            let mut full_buffer: Vec<u8> = vec![];
            let mut buffer = vec![0u8; 1000]; // this needs to be set such that we'll be able to get "Duration unbroken" (frame should be ok)

            log::info!(target: LOG_TARGET_STDERR, "{} Entering STDERR read loop...", id_stderr);
            loop {
                match stderr.read(&mut buffer) {
                    Ok(n) => {
                        log::debug!(target: LOG_TARGET_STDERR, "{} {} bytes read.", id_stderr, n);

                        log::debug!(target: LOG_TARGET_STDERR, "{} Trying to acquire 'job cancelled' mutex to make sure job has not been cancelled...", id_stderr);
                        let job_cancelled = {
                            let job_cancelled = match job_cancelled_stderr.lock() {
                                Ok(m) => {
                                    log::debug!(target: LOG_TARGET_STDERR, "{} Successfully acquired 'job cancelled' mutex.", id_stderr);
                                    m
                                }
                                Err(e) => {
                                    log::error!(target: LOG_TARGET_STDERR, "{} Failed to acquire 'job cancelled' mutex: {:?}", id_stderr, e);
                                    panic!();
                                }
                            };
                            *job_cancelled
                        };
                        if job_cancelled {
                            log::info!(target: LOG_TARGET_STDERR, "{} Job has been cancelled, so breaking out of loop...", id_stderr);
                            break;
                        } else {
                            log::debug!(target: LOG_TARGET_STDERR, "{} Job has not been cancelled, so carrying on with output parsing...", id_stderr);
                        }

                        if n > 0 {
                            full_buffer.append(&mut buffer[..n].to_vec());

                            if duration.is_none() {
                                log::debug!(target: LOG_TARGET_STDERR, "{} Trying to parse buffer into string...", id_stderr);
                                let s = match std::str::from_utf8(&full_buffer[..]) {
                                    Ok(s) => {
                                        log::debug!(target: LOG_TARGET_STDERR, "{} Successfully parsed buffer into string.", id_stderr);
                                        log::trace!(target: LOG_TARGET_STDERR, "{} Logging parsed buffer:\n{}", id_stderr, s);
                                        s
                                    }
                                    Err(e) => {
                                        log::error!(target: LOG_TARGET_STDERR, "{} Failed to parse buffer into string: {:?}", id_stderr, e);
                                        panic!();
                                    }
                                };
                                log::debug!(target: LOG_TARGET_STDERR, "{} Trying to extract video duration from parsed string...", id_stderr);
                                if let Some(d) = try_extract_duration(s, Some(&id_stderr_string)) {
                                    log::info!(target: LOG_TARGET_STDERR, "{} Video duration successfully extracted: {:?}", id_stderr, d);
                                    duration = Some(d);
                                    log::debug!(target: LOG_TARGET_STDERR, "{} Trying to send video duration down channel...", id_stderr);
                                    match tx_stderr.send(Message::VideoDuration(d)) {
                                        Ok(_) => {
                                            log::debug!(target: LOG_TARGET_STDERR, "{} Video duration successfully sent down channel.", id_stderr);
                                        }
                                        Err(e) => {
                                            log::error!(target: LOG_TARGET_STDERR, "{} Failed to send video duration down channel: {:?}", id_stderr, e);
                                            panic!();
                                        }
                                    }
                                }
                            }

                            log::debug!(target: LOG_TARGET_STDERR, "{} Trying to parse buffer into string...", id_stderr);
                            let s = match std::str::from_utf8(&buffer[..n]) {
                                Ok(s) => {
                                    log::debug!(target: LOG_TARGET_STDERR, "{} Successfully parsed buffer into string.", id_stderr);
                                    log::trace!(target: LOG_TARGET_STDERR, "{} Logging parsed buffer:\n{}", id_stderr, s);
                                    s
                                }
                                Err(e) => {
                                    log::error!(target: LOG_TARGET_STDERR, "{} Failed to parse buffer into string: {:?}", id_stderr, e);
                                    panic!();
                                }
                            };

                            if s.starts_with("frame=") {
                                log::debug!(target: LOG_TARGET_STDERR, "{} Parsed string starts with 'frame=', so trying to extra frame time from it...", id_stderr);
                                if let Some(time) =
                                    try_extract_frame_time(s, Some(&id_stderr_string))
                                {
                                    log::debug!(target: LOG_TARGET_STDERR, "{} Successfully extracted 'time' from string: {:?}", id_stderr, time);
                                    if let Some(duration) = duration {
                                        let progress = progress_from_durations(duration, time);
                                        log::info!(target: LOG_TARGET_STDERR, "{} New progress calculated: {:.04}", id_stderr, progress);
                                        log::debug!(target: LOG_TARGET_STDERR, "{} Trying to send newly calculated progress down channel...", id_stderr);
                                        match tx_stderr.send(Message::Progress(progress)) {
                                            Ok(_) => {
                                                log::debug!(target: LOG_TARGET_STDERR, "{} Successfully sent newly calculated progress down channel.", id_stderr);
                                            }
                                            Err(e) => {
                                                log::error!(target: LOG_TARGET_STDERR, "{} Failed to send newly calculated progress down channel: {:?}", id_stderr, e);
                                                panic!();
                                            }
                                        }
                                    }
                                } else {
                                    // So this is possible if we input an invalid file (e.g. a png), in which case we will get something
                                    // similar to this (i.e. a "frame=" without first a duration):
                                    //     out#0/gif @ 0x7fe0a5714b00] Error writing trailer: Invalid argumentbitrate=  -0.0kbits/s speed=N/A
                                    //         frame=    0 fps=0.0 q=0.0 Lsize=       0kB time=-577014:32:22.77 bitrate=  -0.0kbits/s speed=N/A

                                    // NOTE: No need to panic here I think. We can just do nothing. If it
                                    // was an invalid input, `stderr` will close and the loop will automatically
                                    // break...
                                    log::warn!(target: LOG_TARGET_STDERR, "{} NOTE: frame= received without duration parsed. This may have been caused by invalid input file type.", id_stderr);
                                }
                            }
                        } else {
                            log::info!(target: LOG_TARGET_STDERR, "{} No more data to read. Breaking out of STDERR thread loop...", id_stderr);
                            break;
                        }
                    }
                    Err(e) => {
                        if let std::io::ErrorKind::WouldBlock = e.kind() {
                        } else {
                            log::error!(target: LOG_TARGET_STDERR, "{} Error reading STDERR: {:?}", id_stderr, e);
                            panic!();
                        }
                    }
                }
            }

            log::info!(target: LOG_TARGET_STDERR, "{} Exiting STDERR thread...", id_stderr);
        });

        let tx_child = self.tx.clone();
        let id_child = self.id();
        let handle_child = std::thread::spawn(move || {
            log::info!(target: LOG_TARGET_CHILD, "{} Entered CHILD process thread", id_child);

            log::debug!(target: LOG_TARGET_CHILD, "{} Calling 'wait' method on the child process instance...", id_child);
            match child.wait() {
                Ok(status) => {
                    log::info!(target: LOG_TARGET_CHILD, "{} Child process completed with exit status: {:?} (exit code: {:?})", id_child, status, status.code());
                    if let Some(code) = status.code() {
                        if code > 0 {
                            log::debug!(target: LOG_TARGET_CHILD, "{} Trying to send error message down channel...", id_child);
                            match tx_child.send(Message::Error(Error::ExitCode(code))) {
                                Ok(_) => {
                                    log::debug!(target: LOG_TARGET_CHILD, "{} Successfully sent error message down channel", id_child);
                                }
                                Err(e) => {
                                    log::error!(target: LOG_TARGET_CHILD, "{} Failed to send error message down channel: {:?}", id_child, e);
                                    panic!();
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!(target: LOG_TARGET_CHILD, "{} Child process error: {:?}", id_child, e);
                    log::debug!(target: LOG_TARGET_CHILD, "{} Trying to send child process error down channel...", id_child);
                    match tx_child.send(Message::Error(Error::ChildProcess(std::sync::Arc::new(e))))
                    {
                        Ok(_) => {
                            log::debug!(target: LOG_TARGET_CHILD, "{} Successfully sent child process error down channel.", id_child);
                        }
                        Err(e) => {
                            log::error!(target: LOG_TARGET_CHILD, "{} Failed to send child process error down channel: {:?}", id_child, e);
                            panic!();
                        }
                    }
                }
            }

            log::info!(target: LOG_TARGET_CHILD, "{} Exiting CHILD process thread...", id_child);
        });

        log::debug!(target: LOG_TARGET_MAIN, "{} All threads spawned. Now trying to join them sequentially in the following order: child process, stderr, stdout, stdin...", self.id());

        log::debug!(target: LOG_TARGET_MAIN, "{} Trying to join CHILD process thread...", self.id());
        match handle_child.join() {
            Ok(_) => {
                log::debug!(target: LOG_TARGET_MAIN, "{} Successfully joined CHILD process thread", self.id());
            }
            Err(e) => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to join CHILD process thread: {:?}", self.id(), e);
                panic!();
            }
        }
        log::debug!(target: LOG_TARGET_MAIN, "{} Trying to join STDERR thread...", self.id());
        match handle_stderr.join() {
            Ok(_) => {
                log::debug!(target: LOG_TARGET_MAIN, "{} Successfully joined STDERR thread", self.id());
            }
            Err(e) => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to join STDERR thread: {:?}", self.id(), e);
                panic!();
            }
        }
        log::debug!(target: LOG_TARGET_MAIN, "{} Trying to join STDOUT thread...", self.id());
        match handle_stdout.join() {
            Ok(_) => {
                log::debug!(target: LOG_TARGET_MAIN, "{} Successfully joined STDOUT thread", self.id());
            }
            Err(e) => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to join STDOUT thread: {:?}", self.id(), e);
                panic!();
            }
        }
        log::debug!(target: LOG_TARGET_MAIN, "{} Trying to join STDIN thread...", self.id());
        match handle_stdin.join() {
            Ok(_) => {
                log::debug!(target: LOG_TARGET_MAIN, "{} Successfully joined STDIN thread", self.id());
            }
            Err(e) => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to join STDIN thread: {:?}", self.id(), e);
                panic!();
            }
        }

        log::info!(target: LOG_TARGET_MAIN, "{} Trying to send 'done' message down channel...", self.id());
        match self.tx.send(Message::Done) {
            Ok(_) => {
                log::info!(target: LOG_TARGET_MAIN, "{} Successfully sent 'done' message down channel.", self.id());
            }
            Err(e) => {
                log::error!(target: LOG_TARGET_MAIN, "{} Failed to send 'done' message down channel: {:?}", self.id(), e);
                panic!();
            }
        }

        log::info!(target: LOG_TARGET_MAIN, "{} End of 'convert' method reached.", self.id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_logging() {
        std::env::set_var("RUST_LOG", "debug");
        // std::env::set_var("RUST_LOG", "info");
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[cfg(not(feature = "tokio"))]
    #[test]
    fn test_fake() {
        init_logging();
        let settings = Settings::with_standard_fps("./assets/big-buck-bunny-clip.mp4".into(), 200);
        log::info!(
            "Please implement testing for 'default' feature flag... {:?}",
            settings
        );
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn test_converter_blocking() {
        init_logging();

        let settings = Settings::with_standard_fps("./assets/big-buck-bunny-clip.mp4".into(), 200);
        // let settings = Settings::with_standard_fps("./CHANGELOG".into(), 200);
        // let settings = Settings::with_standard_fps("./non-existing-file".into(), 200);

        // NOTE: You can use this to specify the FFmpeg's binary path if it's not
        // on your path.
        // let settings = settings.ffmpeg_path("/usr/local/bin/ffmpeg");

        let (converter, tx, mut rx) = Converter::new_with_channels();
        log::info!("NOTE: Setting 'tx' (i.e. the command sender) above as '_' drops it right away and does not allow us to confirm that the STDIN thread works as expected, so keeping a reference to keep it alive and printting it to get rid of warning: {:?}", tx);

        let thread_handle = std::thread::spawn(move || {
            converter.convert(settings);
        });

        loop {
            match rx.blocking_recv() {
                Some(message) => match message {
                    Message::Done => {
                        log::info!(
                            "Received DONE message from converter. So breaking out of loop..."
                        );
                        break;
                    }
                    Message::Error(e) => {
                        log::warn!("{:?}", e);
                    }
                    Message::Progress(progress) => {
                        log::info!("Progress received: {:.04}", progress);
                    }
                    Message::VideoDuration(duration) => {
                        log::info!("Duration received: {:?}", duration);
                    }
                    Message::Success(data) => {
                        log::info!("Successfully parsed data. Byte-length = {}", data.len());
                    }
                },
                None => {
                    break;
                }
            }
        }

        thread_handle
            .join()
            .expect("Failed to join converter thread");
    }
    #[cfg(feature = "tokio")]
    #[test]
    fn test_converter_blocking_cancelled_job() {
        init_logging();

        let settings = Settings::with_standard_fps("./assets/big-buck-bunny-clip.mp4".into(), 400);

        let (converter, tx, mut rx) = Converter::new_with_channels();

        let convert_thread_handle = std::thread::spawn(move || {
            converter.convert(settings);
        });

        let tx_clone_cancel_job = tx.clone();
        let cancel_job_thread_handle = std::thread::spawn(move || {
            const SLEEP_MS: u64 = 1200;
            log::info!(
                "Entering 'cancel job' thread and sleeping for {} milliseconds",
                SLEEP_MS
            );
            std::thread::sleep(std::time::Duration::from_millis(SLEEP_MS));
            log::info!("Cancelling the job...");
            match tx_clone_cancel_job.send(Command::Cancel) {
                Err(e) => {
                    log::warn!(
                        "Failed to send job cancellation command down channel: {:?}",
                        e
                    );
                }
                Ok(_) => {
                    log::info!("Job cancellation command successfully sent down channel.");
                }
            }
        });

        loop {
            match rx.blocking_recv() {
                Some(message) => match message {
                    Message::Done => {
                        log::info!(
                            "Received DONE message from converter. So breaking out of loop..."
                        );
                        break;
                    }
                    Message::Error(e) => {
                        log::warn!("{:?}", e);
                    }
                    Message::Progress(progress) => {
                        log::info!("Progress received: {:.04}", progress);
                    }
                    Message::VideoDuration(duration) => {
                        log::info!("Duration received: {:?}", duration);
                    }
                    Message::Success(data) => {
                        log::info!("Successfully parsed data. Byte-length = {}", data.len());
                    }
                },
                None => {
                    break;
                }
            }
        }

        convert_thread_handle
            .join()
            .expect("Failed to join converter thread");

        cancel_job_thread_handle
            .join()
            .expect("Failed to join job cancellation thread");
    }
}
