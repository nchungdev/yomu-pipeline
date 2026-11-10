use crate::contract::v1::{GlossaryEntry, TranslateOptions, SCHEMA_VERSION};

/// Computes a stable blake3 hash of a series glossary (ADR-0002 section 7.2).
///
/// Entries are sorted by `(source, target)` so order of insertion does not affect the hash.
pub fn glossary_hash(glossary: &[GlossaryEntry]) -> String {
    if glossary.is_empty() {
        return blake3::hash(b"empty_glossary").to_hex().to_string();
    }
    let mut sorted = glossary.to_vec();
    sorted.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.target.cmp(&b.target))
    });

    let mut hasher = blake3::Hasher::new();
    for entry in &sorted {
        hasher.update(entry.source.as_bytes());
        hasher.update(b"\0");
        hasher.update(entry.target.as_bytes());
        hasher.update(b"\0");
        if let Some(cat) = entry.category {
            hasher.update(format!("{cat:?}").as_bytes());
        }
        hasher.update(b"\0");
        if let Some(ref notes) = entry.notes {
            hasher.update(notes.as_bytes());
        }
        hasher.update(b"\x1e"); // Record separator
    }
    hasher.finalize().to_hex().to_string()
}

/// Computes the cache key of a page under given translation options (ADR-0002 line 102).
///
/// Format: `blake3(source_hash ‖ provider ‖ model ‖ prompt_version ‖ target_lang ‖ glossary_hash ‖ SCHEMA_VERSION)`
pub fn cache_key(source_hash: &str, options: &TranslateOptions) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(source_hash.as_bytes());
    hasher.update(b"|");

    let (provider_str, model_str) = if let Some(first) = options.chain.first() {
        (format!("{:?}", first.kind), first.model.clone())
    } else {
        (options.provider.clone(), String::new())
    };

    hasher.update(provider_str.as_bytes());
    hasher.update(b"|");
    hasher.update(model_str.as_bytes());
    hasher.update(b"|");
    hasher.update(b"v1"); // Prompt schema version
    hasher.update(b"|");
    hasher.update(options.target_lang.as_bytes());
    hasher.update(b"|");
    let gh = glossary_hash(&options.glossary);
    hasher.update(gh.as_bytes());
    hasher.update(b"|");
    hasher.update(SCHEMA_VERSION.to_string().as_bytes());

    hasher.finalize().to_hex().to_string()
}
