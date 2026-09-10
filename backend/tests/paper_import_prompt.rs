use backend::features::paper_import::llama::OCCURRENCE_EXTRACTION_PROMPT;

#[test]
fn extraction_prompt_uses_english_country_and_local_language_locality() {
    assert!(
        OCCURRENCE_EXTRACTION_PROMPT
            .contains("country は locality の言語にかかわらず、英語の一般的な国名で出力")
    );
    assert!(OCCURRENCE_EXTRACTION_PROMPT.contains(
        "可能な限りその地点が属する国・地域で実際に使われる公用語または現地語の地名表記"
    ));
    assert!(OCCURRENCE_EXTRACTION_PROMPT.contains("英語名・ローマ字表記しかなくても"));
    assert!(OCCURRENCE_EXTRACTION_PROMPT.contains("曖昧、候補が複数、または確信が低い場合"));
    assert!(OCCURRENCE_EXTRACTION_PROMPT.contains("元資料が示す地理的な粒度を変えてはいけません"));
}
