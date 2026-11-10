//! Scrubbing of provider-supplied text before it is stored in a `ProviderError`.

const MAX_CHARS: usize = 512;

/// Collapses whitespace, replaces URLs and data URIs, redacts key-looking tokens, then truncates
/// to 512 characters.
pub(crate) fn sanitize_message(raw: &str) -> String {
    let mut out = String::new();
    for (i, tok) in raw.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&sanitize_token(tok));
        if out.chars().count() > MAX_CHARS {
            break;
        }
    }
    out.chars().take(MAX_CHARS).collect()
}

fn sanitize_token(tok: &str) -> String {
    let lower = tok.to_ascii_lowercase();
    if tok.contains("://") || lower.starts_with("www.") {
        return "[url]".to_owned();
    }
    if lower.starts_with("data:") {
        return "[data]".to_owned();
    }
    let mut out = String::new();
    let mut run = String::new();
    for c in tok.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            run.push(c);
        } else {
            flush(&mut out, &mut run);
            out.push(c);
        }
    }
    flush(&mut out, &mut run);
    out
}

fn flush(out: &mut String, run: &mut String) {
    if is_secret(run) {
        out.push_str("[redacted]");
    } else {
        out.push_str(run);
    }
    run.clear();
}

fn is_secret(run: &str) -> bool {
    let len = run.len();
    if run.starts_with("AIza") && len >= 20 {
        return true;
    }
    if run.starts_with("sk-") && len >= 16 {
        return true;
    }
    len >= 32
        && run.bytes().any(|b| b.is_ascii_digit())
        && run.bytes().any(|b| b.is_ascii_alphabetic())
}
