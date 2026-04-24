use sha2::{Digest, Sha256};

/// Create a safe filename slug from chat titles or messages
pub fn slugify(text: &str) -> String {
    // Take first 50 chars
    let truncated: String = text.chars().take(50).collect();

    let slug: String = truncated
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    // Collapse multiple hyphens and trim
    let mut clean_slug = String::new();
    let mut last_was_hyphen = true; // Start true to trim leading hyphens

    for c in slug.chars() {
        if c == '-' {
            if !last_was_hyphen {
                clean_slug.push('-');
                last_was_hyphen = true;
            }
        } else {
            clean_slug.push(c);
            last_was_hyphen = false;
        }
    }

    // Trim trailing hyphen
    if clean_slug.ends_with('-') {
        clean_slug.pop();
    }

    if clean_slug.is_empty() {
        "new-chat".to_string()
    } else {
        clean_slug
    }
}

/// Create a short stable fingerprint for distinguishing otherwise identical filenames.
pub fn short_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();

    digest
        .iter()
        .take(6)
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Who are you?"), "who-are-you");
        assert_eq!(slugify("Hello   World"), "hello-world");
        assert_eq!(slugify("!@#$"), "new-chat");
        assert_eq!(slugify("Simple"), "simple");
    }

    #[test]
    fn test_short_hash_is_stable_and_short() {
        assert_eq!(short_hash("session-1"), short_hash("session-1"));
        assert_ne!(short_hash("session-1"), short_hash("session-2"));
        assert_eq!(short_hash("session-1").len(), 12);
    }
}
