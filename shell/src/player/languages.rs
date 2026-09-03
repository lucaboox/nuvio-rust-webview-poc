/// Resolve Nuvio's language choices before passing ISO language tags to mpv.
/// Mirrors PlayerLanguagePreferences.kt: Original falls back to Device when
/// metadata has no language, while Default leaves selection to the file.
pub fn preferred_languages(
    primary: &str,
    secondary: &str,
    device: &[String],
    original: Option<&str>,
) -> Vec<String> {
    fn code(value: &str) -> Option<String> {
        let value = value.trim().replace('_', "-").to_ascii_lowercase();
        if matches!(
            value.as_str(),
            "" | "device" | "default" | "original" | "none" | "forced"
        ) {
            return None;
        }
        // This is an mpv comma-separated option, not an arbitrary command.
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            .then_some(value)
    }
    let original = original.and_then(code);
    let resolve = |value: &str| {
        if value.trim().eq_ignore_ascii_case("original") {
            original.clone()
        } else {
            code(value)
        }
    };
    let primary = primary.trim().to_ascii_lowercase();
    let mut targets = match primary.as_str() {
        "" | "device" => device.iter().filter_map(|value| code(value)).collect(),
        "original" => original
            .clone()
            .map(|value| vec![value])
            .unwrap_or_else(|| device.iter().filter_map(|value| code(value)).collect()),
        _ => resolve(&primary).into_iter().collect::<Vec<_>>(),
    };
    targets.extend(resolve(secondary));
    let mut result = Vec::new();
    for target in targets {
        // Official track matching accepts a regional tag's base language too.
        // Supply that fallback explicitly for mpv (e.g. en-US then en).
        let base = target.split('-').next().unwrap_or(&target).to_string();
        for candidate in [target, base] {
            if !result.contains(&candidate) {
                result.push(candidate);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::preferred_languages;

    #[test]
    fn device_language_precedes_secondary_and_is_deduplicated() {
        assert_eq!(
            preferred_languages("device", "en", &["en-US".into(), "en".into()], None),
            vec!["en-us", "en"]
        );
    }

    #[test]
    fn original_uses_metadata_then_device_fallback() {
        assert_eq!(
            preferred_languages("original", "en", &["de".into()], Some("ja")),
            vec!["ja", "en"]
        );
        assert_eq!(
            preferred_languages("original", "en", &["de".into()], None),
            vec!["de", "en"]
        );
    }

    #[test]
    fn default_preserves_file_choice_unless_secondary_is_explicit() {
        assert!(preferred_languages("default", "none", &["en".into()], Some("ja")).is_empty());
        assert_eq!(
            preferred_languages("default", "en", &["de".into()], None),
            vec!["en"]
        );
        assert_eq!(
            preferred_languages("default", "original", &[], Some("ja")),
            vec!["ja"]
        );
    }

    #[test]
    fn explicit_language_wins_and_sentinels_never_reach_mpv() {
        assert_eq!(
            preferred_languages("fr", "en", &["de".into()], None),
            vec!["fr", "en"]
        );
        assert!(preferred_languages("none", "forced", &[], None).is_empty());
        assert!(preferred_languages("en,fr", "none", &[], None).is_empty());
    }
}
