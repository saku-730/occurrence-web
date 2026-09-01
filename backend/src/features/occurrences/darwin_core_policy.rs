use std::{collections::HashSet, sync::LazyLock};

const DARWIN_CORE_TERM_LIST_CSV: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../frontend/content/terms/darwin-core/list.csv"
));

static SUPPORTED_DARWIN_CORE_IRIS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut lines = DARWIN_CORE_TERM_LIST_CSV.lines();
    let header = parse_csv_row(
        lines
            .next()
            .expect("Darwin Core list.csv must have a header"),
    );

    let namespace_index = header
        .iter()
        .position(|field| field == "namespace")
        .expect("Darwin Core list.csv must contain namespace");
    let iri_index = header
        .iter()
        .position(|field| field == "iri")
        .expect("Darwin Core list.csv must contain iri");
    let enabled_index = header
        .iter()
        .position(|field| field == "use_at_bio_database")
        .expect("Darwin Core list.csv must contain use_at_bio_database");

    lines
        .filter_map(|line| {
            let fields = parse_csv_row(line);
            let namespace = fields.get(namespace_index)?;
            let iri = fields.get(iri_index)?;
            let enabled = fields.get(enabled_index)?;

            (namespace == "dwc" && enabled.eq_ignore_ascii_case("true")).then(|| iri.clone())
        })
        .collect()
});

pub fn is_supported_darwin_core_iri(iri: &str) -> bool {
    SUPPORTED_DARWIN_CORE_IRIS.contains(iri)
}

fn parse_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => fields.push(std::mem::take(&mut field)),
            _ => field.push(ch),
        }
    }

    fields.push(field);
    fields
}

#[cfg(test)]
mod tests {
    use super::is_supported_darwin_core_iri;

    #[test]
    fn active_darwin_core_term_is_supported() {
        assert!(is_supported_darwin_core_iri(
            "http://rs.tdwg.org/dwc/terms/scientificName"
        ));
    }

    #[test]
    fn inactive_darwin_core_term_is_not_supported() {
        assert!(!is_supported_darwin_core_iri(
            "http://rs.tdwg.org/dwc/terms/acceptedScientificName"
        ));
    }

    #[test]
    fn non_darwin_core_namespace_is_not_supported_by_this_endpoint() {
        assert!(!is_supported_darwin_core_iri(
            "http://purl.org/dc/terms/license"
        ));
    }
}
