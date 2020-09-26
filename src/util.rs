pub fn bytevec_to_str(input: &[u8]) -> String {
    String::from_utf8_lossy(input).to_string()
}

pub fn only_allowed_chars_not_empty(input: &str, allowed: &str) -> bool {
    !input.is_empty() && input.chars().all(|c| allowed.contains(c))
}
