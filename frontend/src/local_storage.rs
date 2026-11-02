//! A value the browser keeps for this user alone, never sent to the backend or seen by anyone
//! else; used for a page's own, unpublished preferences and overrides.

/// `key`'s value; none where the browser keeps nothing (a private window, blocked storage).
pub fn stored(key: &str) -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
}

pub fn store(key: &str, value: &str) {
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.set_item(key, value);
    }
}

pub fn remove(key: &str) {
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.remove_item(key);
    }
}
