# Animated GIF Generator Rust Library Based On A Simple FFmpeg Child Process Wrapper

This is a simple, very experimental Rust library that makes a system call to `FFmpeg` to generate an animated GIF from a video path.

## Disclaimer

This project is still (and will likely always remain) in an early experimental state and was not thoroughly tested. I created this project as part of another toy project (_add link here_) and stopped as soon as I had a working prototype on my development system (i.e. macOS).

## Requirements

* The library assumes that the system has `ffmpeg version 5.0-tessus` installed on its path. You may run `ffmpeg -version` in a terminal to confirm that. See [FFmpeg - Downloads](https://ffmpeg.org/download.html#releases) if you need to install it. It is possible that the library will work with other versions, but this was not tested.

## Feature flags

The library relies on `mpsc` channels for communication between threads. You can use the `default` (or, equivalently, no flag at all) feature flag to use [std::sync::mpsc](https://doc.rust-lang.org/std/sync/mpsc/index.html) channels, or use the `tokio` feature flag to instead use the [tokio::sync::mpsc](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) unbounded channels. The `tokio` channels are allowed to be send between asynchronous tasks, which may be a requirement for some applications.

### Feature flags and documentation

To view the documentation for the `default` feature flag (or no flag at all), run `cargo doc --features default --no-deps --open` in a terminal; to view the documentation for the `tokio` feature flag, run `cargo doc --features tokio --no-deps --open` in a terminal.

## Examples

The [./examples](./examples) directory contains two examples that illustrate how the library can be used. One example uses blocking calls on the receiver's end, while the other example uses non blocking calls. Both examples require the `tokio` flag. The reason for which there is no example using the `default` flag is simply because I haven't been able to configure the `rust-analyzer` in a way that it wouldn't complain with examples of both types in the same workspace (i.e. I can only either specify `tokio` or `default` in [./.vscode/settings.json](./.vscode/settings.json)).

So, here's how to run the examples:
* `cargo run --features tokio --example how_to`
* `cargo run --features tokio --example how_to_async`

You could also run them in release mode by adding the `--release` flag like this: `cargo run --release ...`

You may also checkout my other repository for an example illustrating how the library is used in a simple GIF maker GUI application. **(link to repo coming soon...)**

## Other useful links and references

* Check out [How to Make a GIF from a Video using FFmpeg](https://creatomate.com/blog/how-to-make-a-gif-from-a-video-using-ffmpeg) for a nice article on how to convert a video to an animated GIF using FFmpeg.

## Contact

Please feel free to contact me by opening an issue on the [repository](https://github.com/BB-301/rust-ffmpeg-gif-maker/issues) if you have any questions, issues or suggestions for this project.

## License

I am releasing this project under the [MIT License](./LICENSE).