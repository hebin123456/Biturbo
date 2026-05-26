use std::cell::RefCell;

thread_local! {
    static LAST_ERROR: RefCell<Option<Vec<u8>>> = RefCell::new(None);
}

pub fn set_last_error_str(msg: &str) {
    LAST_ERROR.with(|e| *e.borrow_mut() = Some(msg.as_bytes().to_vec()));
}

pub fn take_last_error_bytes() -> Option<Vec<u8>> {
    LAST_ERROR.with(|e| e.borrow_mut().take())
}

