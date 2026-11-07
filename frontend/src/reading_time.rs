//! Scope: When a reading was taken, from the age the backend sends, and its time of day; not
//! fetching it (`graphql`).
/// When something was read that is `age` seconds old (milliseconds since the epoch): ages come
/// from the server, times are the browser's.
pub fn read_at(age: i32) -> f64 {
    js_sys::Date::now() - f64::from(age) * 1000.0
}

/// The time `new` of an answer again, but `before` where it is the same reading: an age comes
/// in whole seconds, so [`read_at`] of one reading moves by up to a second between answers.
pub fn same_reading(before: Option<f64>, new: f64) -> f64 {
    before
        .filter(|before| (before - new).abs() < 1500.0)
        .unwrap_or(new)
}

/// `HH:MM:SS` of `millis` since the epoch, in the browser's time zone.
pub fn local_time(millis: f64) -> String {
    let date = js_sys::Date::new(&millis.into());
    format!(
        "{:02}:{:02}:{:02}",
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds()
    )
}
