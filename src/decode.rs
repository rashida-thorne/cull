//! Byte-to-text decoding for HTML documents.
//!
//! Real-world pages are not all UTF-8. We follow (a pragmatic subset of) the
//! WHATWG encoding-sniffing algorithm, in priority order:
//!
//! 1. Byte-order mark (UTF-8, UTF-16 LE/BE) — always wins.
//! 2. `charset` parameter from the HTTP `Content-Type` header, if any.
//! 3. A `<meta charset=…>` / `<meta http-equiv="content-type" …>` declaration
//!    sniffed from the first 1024 bytes of the document.
//! 4. Fall back to UTF-8 (lossy — invalid sequences become U+FFFD).

use encoding_rs::Encoding;

/// Decode raw HTML bytes into a `String`.
///
/// `header_charset` is the charset label from an HTTP `Content-Type` header,
/// when the input came from a URL fetch.
pub fn decode_html(bytes: &[u8], header_charset: Option<&str>) -> String {
    // 1. BOM sniffing (also strips the BOM).
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return text.into_owned();
    }

    // 2. HTTP header charset.
    if let Some(enc) = header_charset.and_then(|label| Encoding::for_label(label.trim().as_bytes()))
    {
        let (text, _) = enc.decode_without_bom_handling(bytes);
        return text.into_owned();
    }

    // 3. <meta> prescan of the first 1024 bytes.
    if let Some(enc) = sniff_meta_charset(&bytes[..bytes.len().min(1024)])
        .and_then(|label| Encoding::for_label(label.as_bytes()))
    {
        let (text, _) = enc.decode_without_bom_handling(bytes);
        return text.into_owned();
    }

    // 4. Default: UTF-8, lossy.
    String::from_utf8_lossy(bytes).into_owned()
}

/// Extract the charset label from a `Content-Type` header value, e.g.
/// `text/html; charset=windows-1251` → `Some("windows-1251")`.
pub fn charset_from_content_type(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let idx = lower.find("charset=")?;
    let rest = &value[idx + "charset=".len()..];
    let rest = rest.trim_start();
    let label = if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next().unwrap_or("")
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        stripped.split('\'').next().unwrap_or("")
    } else {
        rest.split([';', ' ', '\t']).next().unwrap_or("")
    };
    let label = label.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

/// Scan a byte prefix for `charset=` inside a `<meta …>` tag.
///
/// This is a simplified version of the WHATWG prescan: we look for
/// `charset` followed by `=` and a (possibly quoted) label, but only inside
/// `<meta` tags, and we skip comments. Good enough for documents in the wild.
fn sniff_meta_charset(bytes: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // Skip comments entirely so a commented-out meta doesn't win.
        if bytes[i..].starts_with(b"<!--") {
            let end = find_sub(&bytes[i + 4..], b"-->")?;
            i += 4 + end + 3;
            continue;
        }
        if !starts_with_ci(&bytes[i + 1..], b"meta") {
            i += 1;
            continue;
        }
        // Find end of this tag.
        let tag_end = find_byte(&bytes[i..], b'>').map(|e| i + e)?;
        let tag = &bytes[i..tag_end];
        if let Some(label) = charset_in_tag(tag) {
            return Some(label);
        }
        i = tag_end + 1;
    }
    None
}

fn charset_in_tag(tag: &[u8]) -> Option<String> {
    let lower: Vec<u8> = tag.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut from = 0;
    while let Some(pos) = find_sub(&lower[from..], b"charset") {
        let mut j = from + pos + "charset".len();
        while j < tag.len() && tag[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= tag.len() || tag[j] != b'=' {
            from += pos + "charset".len();
            continue;
        }
        j += 1;
        while j < tag.len() && tag[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= tag.len() {
            return None;
        }
        let label_bytes = match tag[j] {
            q @ (b'"' | b'\'') => {
                let start = j + 1;
                let end = find_byte(&tag[start..], q).map(|e| start + e)?;
                &tag[start..end]
            }
            _ => {
                let start = j;
                let mut end = j;
                while end < tag.len()
                    && !tag[end].is_ascii_whitespace()
                    && !matches!(tag[end], b'/' | b'>' | b'"' | b'\'' | b';')
                {
                    end += 1;
                }
                &tag[start..end]
            }
        };
        let label = String::from_utf8_lossy(label_bytes).trim().to_string();
        if !label.is_empty() {
            return Some(label);
        }
        from += pos + "charset".len();
    }
    None
}

fn starts_with_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_passthrough() {
        assert_eq!(decode_html("héllo".as_bytes(), None), "héllo");
    }

    #[test]
    fn bom_utf16le() {
        let mut b = vec![0xFF, 0xFE];
        for u in "hi".encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_html(&b, None), "hi");
    }

    #[test]
    fn bom_beats_header() {
        let mut b = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        b.extend_from_slice("é".as_bytes());
        assert_eq!(decode_html(&b, Some("windows-1251")), "é");
    }

    #[test]
    fn header_charset_wins_over_meta() {
        // "привет" in windows-1251, but meta claims latin-1.
        let mut b = b"<meta charset=\"iso-8859-1\"><p>".to_vec();
        b.extend_from_slice(&[0xEF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
        b.extend_from_slice(b"</p>");
        let out = decode_html(&b, Some("windows-1251"));
        assert!(out.contains("привет"), "{out}");
    }

    #[test]
    fn meta_charset_windows_1251() {
        let mut b = b"<html><head><meta charset=windows-1251></head><body>".to_vec();
        b.extend_from_slice(&[0xEF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]); // привет
        b.extend_from_slice(b"</body></html>");
        let out = decode_html(&b, None);
        assert!(out.contains("привет"), "{out}");
    }

    #[test]
    fn meta_http_equiv_shift_jis() {
        let mut b =
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=Shift_JIS\"><p>"
                .to_vec();
        b.extend_from_slice(&[0x93, 0xFA, 0x96, 0x7B]); // 日本
        let out = decode_html(&b, None);
        assert!(out.contains("日本"), "{out}");
    }

    #[test]
    fn commented_meta_ignored() {
        let b = b"<!-- <meta charset=windows-1251> --><meta charset=utf-8>\xC3\xA9".to_vec();
        assert_eq!(
            decode_html(&b, None),
            "<!-- <meta charset=windows-1251> --><meta charset=utf-8>\u{e9}"
        );
    }

    #[test]
    fn meta_only_in_first_1024_bytes() {
        let mut b = vec![b' '; 1100];
        b.extend_from_slice(b"<meta charset=windows-1251>");
        b.push(0xE9); // invalid UTF-8 alone -> replacement char under fallback
        let out = decode_html(&b, None);
        assert!(
            out.contains('\u{FFFD}'),
            "meta beyond 1024 bytes must be ignored"
        );
    }

    #[test]
    fn content_type_parsing() {
        assert_eq!(
            charset_from_content_type("text/html; charset=UTF-8").as_deref(),
            Some("UTF-8")
        );
        assert_eq!(
            charset_from_content_type("text/html; charset=\"windows-1251\"; foo=bar").as_deref(),
            Some("windows-1251")
        );
        assert_eq!(
            charset_from_content_type("text/html; CHARSET=latin1").as_deref(),
            Some("latin1")
        );
        assert_eq!(charset_from_content_type("text/html"), None);
        assert_eq!(charset_from_content_type("text/html; charset="), None);
    }
}
