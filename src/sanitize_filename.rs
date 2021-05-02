enum State {
    Passed, Skipped
}

fn is_byte_valid_char(c: u8) -> bool {
    if c >= b'a' && c <= b'z' {
        return true;
    }

    if c >= b'A' && c <= b'Z' {
        return true;
    }

    if c >= b'0' && c <= b'9' {
        return true;
    }

    if c == b'.' || c == b'_' {
        return true;
    }

    false
}

pub fn sanitize_filename<T: AsRef<str>>(filename: T) -> String {
    let filename: &[u8] = filename.as_ref().as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(filename.len());
    let mut state = State::Passed;

    for c in filename.iter() {
        let c = *c;
        if is_byte_valid_char(c) {
            out.push(c);
            state = State::Passed;
        } else {
            match state {
                State::Passed => {
                    // replace first invalid character in a row with underscore
                    state = State::Skipped;
                    out.push(b'_');
                },
                State::Skipped => {
                    // skip more than one invalid character in a row
                }
            }
        }
    }

    unsafe {
        String::from_utf8_unchecked(out)
    }

}

#[cfg(test)]
mod tests {
    extern crate test;
    use super::*;

    #[test]
    fn test_valid() {
        assert_eq!(sanitize_filename("test123_2.txt"), "test123_2.txt".to_string())
    }

    #[test]
    fn test_no_good_symbols() {
        assert_eq!(sanitize_filename("🐧ы Ķ"), "_".to_string())
    }

    #[test]
    fn test_mix_valid_and_invalid() {
        assert_eq!(sanitize_filename("hello🐧ыblah Ķ.txt"), "hello_blah_.txt".to_string())
    }
}
