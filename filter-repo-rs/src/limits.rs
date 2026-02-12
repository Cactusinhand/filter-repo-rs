use std::io;

/// Maximum allowed data block size to avoid pathological allocations from
/// malformed fast-export streams.
pub const MAX_DATA_BLOCK_SIZE: usize = 500 * 1024 * 1024; // 500 MB

pub fn parse_data_size_header(line: &[u8]) -> io::Result<usize> {
    let size_bytes = line
        .strip_prefix(b"data ")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
    let n = std::str::from_utf8(size_bytes)
        .ok()
        .map(|s| s.trim())
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
    if n > MAX_DATA_BLOCK_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "blob size {} exceeds maximum allowed size {}",
                n, MAX_DATA_BLOCK_SIZE
            ),
        ));
    }
    Ok(n)
}
