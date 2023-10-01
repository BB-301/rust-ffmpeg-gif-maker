use std::time::Duration;

fn duration_from_ffmpeg_time_string(s: &str) -> Option<Duration> {
    // Expected format:  HH:mm:ss.ms (e.g. 00:00:04.91)

    let dot_splitted: Vec<String> = s.split('.').map(|s| s.to_owned()).collect();
    if dot_splitted.len() != 2 {
        return None;
    }

    let colon_splitted: Vec<String> = dot_splitted[0].split(':').map(|s| s.to_owned()).collect();
    if colon_splitted.len() != 3 {
        return None;
    }

    let Ok(milliseconds) = dot_splitted[1].parse::<u64>() else {
        return None;
    };
    if milliseconds >= 1000 {
        return None;
    }

    let Ok(seconds) = colon_splitted[2].parse::<u64>() else {
        return None;
    };
    if seconds >= 60 {
        return None;
    }

    let Ok(minutes) = colon_splitted[1].parse::<u64>() else {
        return None;
    };
    if minutes >= 60 {
        return None;
    }

    let Ok(hours) = colon_splitted[0].parse::<u64>() else {
        return None;
    };

    let total = milliseconds + (seconds * 1000) + (minutes * 60 * 1000) + (hours * 60 * 60 * 1000);
    Some(Duration::from_millis(total))
}

pub(crate) fn try_extract_frame_time(s: &str) -> Option<Duration> {
    const PATTERN_1: &'static str = "\nframe=";
    const PATTERN_2: &'static str = "time=";
    let splitted = s.split(PATTERN_1);
    if splitted.clone().count() < 1 {
        return None;
    }
    let last = splitted.last().unwrap();
    let Some(time) = last
        .split_ascii_whitespace()
        .find(|s| s.starts_with(PATTERN_2))
    else {
        return None;
    };
    let time = time.replace("time=", "");
    duration_from_ffmpeg_time_string(&time)
}

pub(crate) fn try_extract_duration(s: &str) -> Option<Duration> {
    //  PATTERN:  Duration: 00:00:05.06, start: 0.000000, bitrate: 1785 kb/s
    const PATTERN_1: &'static str = "\n  Duration: ";
    const PATTERN_2: &'static str = ", start: ";
    // NOTE: splitn(1,...) does not mean "split once", it means that it return only one item
    for component in s.splitn(2, PATTERN_1) {
        if component.contains(PATTERN_2) {
            if let Some(time) = component.splitn(2, PATTERN_2).next() {
                return duration_from_ffmpeg_time_string(time);
            }
        }
    }
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

    #[test]
    fn test_try_extract_duration() {
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

        println!("{:?}", try_extract_duration(s));
    }

    #[test]
    fn test_try_extract_frame_time() {
        const FRAME_LINE: &'static str = r#"""
frame=   50 fps=3.9 q=-0.0 Lsize=   23430kB time=00:00:04.91 bitrate=39091.3kbits/s speed=0.379x    
  frame=   50 fps=3.9 q=-0.0 Lsize=   23430kB time=00:00:014.91 bitrate=39091.3kbits/s speed=0.379x    
        """#;
        println!("{:?}", try_extract_frame_time(FRAME_LINE));
    }

    #[test]
    fn test_duration_from_ffmpeg_time_string() {
        let expected = Duration::from_millis(4 * 1000 + 91);
        let calulcated = duration_from_ffmpeg_time_string("00:00:04.91").unwrap();
        assert_eq!(expected, calulcated);
    }
}
