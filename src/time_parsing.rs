use std::time::Duration;

// NOTE: The functions in this file appear to be working for
// parsing the output of `ffmpeg version 5.0-tessus`, but they
// could definitely use more testing.

const LOG_TARGET_FN_DURATION: &'static str = "ffmpeg_gif_maker::time_parser::fn_duration";
const LOG_TARGET_FN_TRY_TIME: &'static str = "ffmpeg_gif_maker::time_parser::fn_try_extract_time";
const LOG_TARGET_FN_TRY_DURATION: &'static str =
    "ffmpeg_gif_maker::time_parser::fn_try_extract_duration";

fn duration_from_ffmpeg_time_string(s: &str, logging_identifier: Option<&str>) -> Option<Duration> {
    // Expected format:  HH:mm:ss.ms (e.g. 00:00:04.91)

    let id = logging_identifier
        .map(|s| format!("{} ", s))
        .unwrap_or("".into());

    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Trying to parse FFmpeg time string into valid duration...", id);
    log::trace!(target: LOG_TARGET_FN_DURATION, "{}Input:\n{}", id, s);

    let dot_splitted: Vec<String> = s.split('.').map(|s| s.to_owned()).collect();
    if dot_splitted.len() != 2 {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Failed to split '.' into two strings.", id);
        return None;
    }

    let colon_splitted: Vec<String> = dot_splitted[0].split(':').map(|s| s.to_owned()).collect();
    if colon_splitted.len() != 3 {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Failed to split ':' into three strings.", id);
        return None;
    }

    let Ok(milliseconds) = dot_splitted[1].parse::<u64>() else {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Failed parse milliseconds.", id);
        return None;
    };
    if milliseconds >= 1000 {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Milliseconds greater than 1000? (value = {}).", id, milliseconds);
        return None;
    }
    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Milliseconds successfully parsed: {}", id, milliseconds);

    let Ok(seconds) = colon_splitted[2].parse::<u64>() else {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Failed parse seconds.", id);
        return None;
    };
    if seconds >= 60 {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Seconds greater than 60? (value = {})", id, seconds);
        return None;
    }
    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Seconds successfully parsed: {}", id, seconds);

    let Ok(minutes) = colon_splitted[1].parse::<u64>() else {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Failed parse minutes.", id);
        return None;
    };
    if minutes >= 60 {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Minutes greater than 60? (value = {})", id, minutes);
        return None;
    }
    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Minutes successfully parsed: {}", id, minutes);

    let Ok(hours) = colon_splitted[0].parse::<u64>() else {
        log::debug!(target: LOG_TARGET_FN_DURATION, "{}Failed parse hours.", id);
        return None;
    };
    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Hours successfully parsed: {}", id, hours);

    let total = milliseconds + (seconds * 1000) + (minutes * 60 * 1000) + (hours * 60 * 60 * 1000);
    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Total duration in milliseconds: {}", id, total);

    let duration = Duration::from_millis(total);
    log::debug!(target: LOG_TARGET_FN_DURATION, "{}Duration instance: {:?}", id, duration);

    Some(duration)
}

pub(crate) fn try_extract_frame_time(
    s: &str,
    logging_identifier: Option<&str>,
) -> Option<Duration> {
    let id = logging_identifier
        .map(|s| format!("{} ", s))
        .unwrap_or("".into());

    log::debug!(target: LOG_TARGET_FN_TRY_TIME, "{}Trying to extract duration from FFmpeg time string...", id);
    log::trace!(target: LOG_TARGET_FN_TRY_TIME, "{}Input:\n{}", id, s);
    const PATTERN_1: &'static str = "\nframe=";
    const PATTERN_2: &'static str = "time=";
    let splitted = s.split(PATTERN_1);
    if splitted.clone().count() < 1 {
        log::debug!(target: LOG_TARGET_FN_TRY_TIME, "{}Failed to split '{}' into more than one component", id, PATTERN_1);
        return None;
    }
    let last = splitted.last().unwrap();
    let Some(time) = last
        .split_ascii_whitespace()
        .find(|s| s.starts_with(PATTERN_2))
    else {
        log::debug!(target: LOG_TARGET_FN_TRY_TIME, "{}Could not find '{}' in any of the splitted components", id, PATTERN_2);
        return None;
    };
    let time = time.replace("time=", "");
    log::debug!(target: LOG_TARGET_FN_TRY_TIME, "{}Time string found: {:?}", id, time);
    duration_from_ffmpeg_time_string(&time, logging_identifier)
}

pub(crate) fn try_extract_duration(s: &str, logging_identifier: Option<&str>) -> Option<Duration> {
    let id = logging_identifier
        .map(|s| format!("{} ", s))
        .unwrap_or("".into());

    log::debug!(target: LOG_TARGET_FN_TRY_DURATION, "{}Trying to extract duration from FFmpeg log string...", id);
    log::trace!(target: LOG_TARGET_FN_TRY_DURATION, "{}Input:\n{}", id, s);
    //  PATTERN:  Duration: 00:00:05.06, start: 0.000000, bitrate: 1785 kb/s
    const PATTERN_1: &'static str = "\n  Duration: ";
    const PATTERN_2: &'static str = ", start: ";
    // NOTE: splitn(1,...) does not mean "split once", it means that it return only one item
    for component in s.splitn(2, PATTERN_1) {
        if component.contains(PATTERN_2) {
            if let Some(time) = component.splitn(2, PATTERN_2).next() {
                log::debug!(target: LOG_TARGET_FN_TRY_DURATION, "{}Time string found: {:?}", id, time);
                return duration_from_ffmpeg_time_string(time, logging_identifier);
            }
        }
    }
    log::debug!(target: LOG_TARGET_FN_TRY_DURATION, "{}Nothing found.", id);
    None
}

pub(crate) fn progress_from_durations(total: Duration, processed: Duration) -> f64 {
    let total = total.as_millis() as f64;
    let processed = processed.as_millis() as f64;
    let progress = processed / total;
    progress.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_logging() {
        std::env::set_var("RUST_LOG", "debug");
        let _ = env_logger::builder().is_test(true).try_init();
    }

    #[test]
    fn test_try_extract_duration() {
        init_logging();

        let s = r#"""
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'assets/flower.mp4':
  Metadata:
    major_brand     : mp42
    minor_version   : 0
    compatible_brands: mp42mp41isomavc1
    creation_time   : 2018-03-07T15:21:21.000000Z
  Duration: 00:00:05.06, start: 0.000000, bitrate: 1785 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, smpte170m, progressive), 960x540 [SAR 1:1 DAR 16:9], 1538 kb/s, 29.97 fps, 29.97 tbr, 30k tbn (default)
    Metadata:
        """#;

        println!("{:?}", try_extract_duration(s, None));
    }

    #[test]
    fn test_try_extract_frame_time() {
        const FRAME_LINE: &'static str = r#"""
frame=   50 fps=3.9 q=-0.0 Lsize=   23430kB time=00:00:04.91 bitrate=39091.3kbits/s speed=0.379x    
  frame=   50 fps=3.9 q=-0.0 Lsize=   23430kB time=00:00:014.91 bitrate=39091.3kbits/s speed=0.379x    
        """#;
        println!("{:?}", try_extract_frame_time(FRAME_LINE, None));
    }

    #[test]
    fn test_duration_from_ffmpeg_time_string() {
        let expected = Duration::from_millis(4 * 1000 + 91);
        let calulcated = duration_from_ffmpeg_time_string("00:00:04.91", None).unwrap();
        assert_eq!(expected, calulcated);
    }
}
