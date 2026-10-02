//! Fitting text into cells.

/// `text`, cut to `width` characters with an ellipsis when it does not fit.
pub fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn long_text_gets_an_ellipsis() {
        assert_eq!(fit("adb-R58M._adb-tls-connect._tcp", 10), "adb-R58M.…");
        assert_eq!(fit("short", 10), "short");
    }
}
