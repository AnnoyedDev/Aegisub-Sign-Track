use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

pub fn set(english: bool) {
    ENGLISH.store(english, Ordering::Relaxed);
}

#[cfg(windows)]
pub fn system() -> bool {
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    let id = unsafe { GetUserDefaultUILanguage() };
    id & 0x3ff != 0x0c
}

#[cfg(not(windows))]
pub fn system() -> bool {
    !["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).is_some_and(|v| v.starts_with("fr"))
}

macro_rules! t {
    ($fr:literal, $en:literal) => {
        if $crate::lang::english() { $en } else { $fr }
    };
}

macro_rules! tf {
    ($fr:literal, $en:literal $($rest:tt)*) => {
        if $crate::lang::english() { format!($en $($rest)*) } else { format!($fr $($rest)*) }
    };
}
