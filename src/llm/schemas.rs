//! Structured-output schemas for the regions payload
//! `{regions:[{id, box_2d, text, translated_text, k, spk, g, age, emo, int}]}`.
//! `box_2d` is `[ymin, xmin, ymax, xmax]` on a 0-1000 scale (see `parse`).

use serde_json::{json, Value};

/// `generationConfig.responseMimeType` to use together with [`gemini_response_schema`].
pub const GEMINI_RESPONSE_MIME_TYPE: &str = "application/json";

const KINDS: [&str; 4] = ["dialogue", "narration", "thought", "sfx"];
const GENDERS: [&str; 3] = ["m", "f", "u"];
const AGES: [&str; 5] = ["child", "teen", "adult", "elder", "u"];
const EMOTIONS: [&str; 8] = [
    "neutral",
    "happy",
    "sad",
    "angry",
    "surprised",
    "serious",
    "whisper",
    "fear",
];

/// Value for Gemini `generationConfig.responseSchema` (upper-case OpenAPI types).
pub fn gemini_response_schema() -> Value {
    json!({
        "type": "OBJECT",
        "properties": {
            "regions": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "id": { "type": "STRING" },
                        "box_2d": {
                            "type": "ARRAY",
                            "items": { "type": "INTEGER" },
                            "minItems": 4,
                            "maxItems": 4
                        },
                        "text": { "type": "STRING" },
                        "translated_text": { "type": "STRING" },
                        "k": { "type": "STRING", "enum": KINDS },
                        "spk": { "type": "STRING", "nullable": true },
                        "g": { "type": "STRING", "enum": GENDERS },
                        "age": { "type": "STRING", "enum": AGES },
                        "emo": { "type": "STRING", "enum": EMOTIONS },
                        "int": { "type": "NUMBER" }
                    },
                    "required": ["id", "box_2d", "text", "translated_text"]
                }
            }
        },
        "required": ["regions"]
    })
}

/// Value for an OpenAI-compatible request's `response_format` (strict `json_schema`).
/// Strict mode needs every property listed in `required`, so optional tags are nullable;
/// enums and array length are left to the parser to keep the schema widely accepted.
pub fn openai_response_format() -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": "comic_page_translation",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "regions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "box_2d": { "type": "array", "items": { "type": "integer" } },
                                "text": { "type": "string" },
                                "translated_text": { "type": "string" },
                                "k": { "type": "string", "enum": KINDS },
                                "spk": { "type": ["string", "null"] },
                                "g": { "type": ["string", "null"] },
                                "age": { "type": ["string", "null"] },
                                "emo": { "type": ["string", "null"] },
                                "int": { "type": ["number", "null"] }
                            },
                            "required": [
                                "id", "box_2d", "text", "translated_text", "k",
                                "spk", "g", "age", "emo", "int"
                            ],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["regions"],
                "additionalProperties": false
            }
        }
    })
}
