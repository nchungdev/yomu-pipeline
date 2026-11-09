use yomu_pipeline::{
    clamp_target_font_size, estimate_target_font_size, is_character_length_balanced,
    vietnamese_break_penalty,
};

#[test]
fn test_estimate_target_font_size_bounds() {
    let s1 = estimate_target_font_size(24.0, 1);
    assert!((s1 - 18.0).abs() < 1e-3);

    // Clamps to min 9.0 px
    let s_small = estimate_target_font_size(10.0, 1);
    assert_eq!(s_small, 9.0);

    // Clamps to max 36.0 px
    let s_large = estimate_target_font_size(200.0, 2);
    assert_eq!(s_large, 36.0);
}

#[test]
fn test_clamp_target_font_size() {
    // Orig estimated 16px -> upper is 16 * 1.15 = 18.4px
    let clamped = clamp_target_font_size(16.0, 24.0);
    assert!((clamped - 18.4).abs() < 1e-2);

    // If max_fitting_size is smaller, it clamps down
    let clamped_tight = clamp_target_font_size(16.0, 14.0);
    assert_eq!(clamped_tight, 14.0);

    // Never below 9px
    let clamped_min = clamp_target_font_size(5.0, 6.0);
    assert_eq!(clamped_min, 9.0);
}

#[test]
fn test_vietnamese_break_penalty_table() {
    // 0: Sentence ending
    assert_eq!(vietnamese_break_penalty("Đúng vậy!", "Chúng ta"), 0.0);
    assert_eq!(vietnamese_break_penalty("Thật sao?", "Tôi"), 0.0);
    assert_eq!(vietnamese_break_penalty("Được rồi...", "Đi thôi"), 0.0);

    // 20: Clause comma
    assert_eq!(vietnamese_break_penalty("Nếu thế,", "thì phải"), 20.0);
    assert_eq!(vietnamese_break_penalty("Này;", "cậu"), 20.0);

    // 40: Conjunction
    assert_eq!(vietnamese_break_penalty("Tôi", "và"), 40.0);
    assert_eq!(vietnamese_break_penalty("Đi", "nhưng"), 40.0);

    // 300: Preposition (của, trong, cho...)
    assert_eq!(vietnamese_break_penalty("Của", "tôi"), 300.0);
    assert_eq!(vietnamese_break_penalty("Trong", "nhà"), 300.0);

    // 2000: Hyphen break
    assert_eq!(vietnamese_break_penalty("T-", "Rex"), 2000.0);

    // 100: Default
    assert_eq!(vietnamese_break_penalty("Xin", "chào"), 100.0);
}

#[test]
fn test_character_length_balanced() {
    // 10 chars vs 12 chars -> ratio 1.2 -> ok
    assert!(is_character_length_balanced("Hello world", "Xin chào bạn"));

    // Extreme expansion / contraction -> false
    assert!(!is_character_length_balanced(
        "Hi",
        "Xin chào các bạn rất nhiều lần nha"
    ));
    assert!(!is_character_length_balanced(
        "Supercalifragilisticexpialidocious",
        "A"
    ));
}
