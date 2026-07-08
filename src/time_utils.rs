pub fn unix_epoch_to_datetime(unix_epoch: u64) -> String {
    chrono::DateTime::from_timestamp(unix_epoch as i64, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

pub fn time_ago(epoch_time: u64) -> String {
    let diff = now() - epoch_time;
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
        .expect("Could not retrieve current time")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_epoch_to_datetime() {
        let dt = chrono::DateTime::from_timestamp(1588888888, 0).unwrap();
        assert_eq!(
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2020-05-07 22:01:28"
        );
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
}
