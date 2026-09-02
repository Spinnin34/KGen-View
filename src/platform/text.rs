pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub fn wide_with_nuls(value: &str) -> Vec<u16> {
    value
        .chars()
        .map(|character| {
            if character == '\0' {
                0
            } else {
                character as u16
            }
        })
        .collect()
}

pub fn nul_pos(buffer: &[u16]) -> usize {
    buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len())
}

pub fn format_bytes(value: u64) -> String {
    if value >= 1024 * 1024 {
        format!("{:.2} MiB", value as f64 / 1024.0 / 1024.0)
    } else if value >= 1024 {
        format!("{:.1} KiB", value as f64 / 1024.0)
    } else {
        format!("{value} B")
    }
}

pub fn format_bytes_decimal(value: u64) -> String {
    if value >= 1024 * 1024 {
        format!("{:.2} MB", value as f64 / 1024.0 / 1024.0)
    } else if value >= 1024 {
        format!("{:.2} KB", value as f64 / 1024.0)
    } else {
        format!("{value} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_file_sizes_compactly() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.00 MiB");
    }
}
