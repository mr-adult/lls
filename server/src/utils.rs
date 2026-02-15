use time::OffsetDateTime;

pub(crate) fn get_iso_string(time_stamp: &OffsetDateTime) -> String {
    format!(
        "{}/{:02}/{:04} at {:02}:{:02}:{:02}.{:03}",
        time_stamp.month() as i32,
        time_stamp.day(),
        time_stamp.year(),
        time_stamp.hour(),
        time_stamp.minute(),
        time_stamp.second(),
        time_stamp.millisecond()
    )
}
