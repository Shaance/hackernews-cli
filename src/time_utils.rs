pub fn unix_epoch_to_datetime(unix_epoch: u64) -> String {
    let Some(epoch) = i64::try_from(unix_epoch).ok() else {
        return "unknown time".to_string();
    };

    match chrono::DateTime::from_timestamp(epoch, 0) {
        Some(datetime) => datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => "unknown time".to_string(),
    }
}

pub fn time_ago(epoch_time: u64) -> String {
    let diff = now().saturating_sub(epoch_time);
    match diff {
        0..=59 => format!("{} seconds ago", diff),
        60..=3599 => format!("{} minutes ago", diff / 60),
        3600..=86399 => format!("{} hours ago", diff / 3600),
        86400..=604799 => format!("{} days ago", diff / 86400),
        _ => format!("{} weeks ago", diff / 604800),
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_epoch_to_datetime() {
        assert_eq!(unix_epoch_to_datetime(1588888888), "2020-05-07 22:01:28");
    }

    #[test]
    fn test_time_ago() {
        let now = now();
        assert_eq!(time_ago(now), "0 seconds ago");
        assert_eq!(time_ago(now - 60), "1 minutes ago");
        assert_eq!(time_ago(now - 3600), "1 hours ago");
        assert_eq!(time_ago(now - 86400), "1 days ago");
        assert_eq!(time_ago(now - 604800), "1 weeks ago");
    }

    #[test]
    fn future_time_is_not_negative() {
        assert_eq!(time_ago(now() + 60), "0 seconds ago");
    }

    #[test]
    fn out_of_range_epoch_is_unknown_instead_of_panicking() {
        assert_eq!(unix_epoch_to_datetime(u64::MAX), "unknown time");
    }
}
