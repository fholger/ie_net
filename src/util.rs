pub fn bytevec_to_str(input: &[u8]) -> String {
    String::from_utf8_lossy(input).to_string()
}
