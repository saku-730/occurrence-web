use oxrdf::{
    GraphName,
    NamedNode,
    Quad,
};
use uuid::Uuid;


#[derive(Debug)]
pub struct CreateOccurrenceInput { //occurrenceデータ作成時の構造体
  //create_user_id:作成者 フロントエンドからとってもいいと思ったが、偽装しやすくなるので。
  //RDF的には、トリプルの中にユーザーidを入れたいと考えるとフロントエンドで組み立てたい。微妙。
    pub create_user_id: Uuid,
    pub content_type: String,
    pub rdf_body: Vec<u8>,
}

#[derive(Debug)]
pub struct CreateOccurrenceOutput {
    pub message: String,
}

#[derive(Debug)]
pub enum OccurrenceServiceError {
    NotImplemented,
    InvalidOccurrenceUri,
    InvalidPredicateUri,
    InvalidUserUri,
    InvalidGraphUri,
}

const CREATOR_PREDICATE_URI: &str = "http://purl.org/dc/terms/creator";
const USER_URI_BASE: &str = "https://bio-database.net/users/";
const OCCURRENCE_GRAPH_URI: &str = "https://bio-database.net/graphs/occurrences";

pub struct OccurrenceService;

impl OccurrenceService {
    pub async fn create_occurrence(
        _input: CreateOccurrenceInput,
    ) -> Result<CreateOccurrenceOutput, OccurrenceServiceError> {
        Err(OccurrenceServiceError::NotImplemented)
    }
}

fn replace_all_subjects_with_occurrence_uri(
    quads: Vec<Quad>,
    occurrence_uri: &str,
) -> Result<Vec<Quad>, OccurrenceServiceError> {
    let occurrence_subject = NamedNode::new(occurrence_uri)
        .map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

    let replaced_quads = quads
        .into_iter()
        .map(|quad| {
            Quad::new(
                occurrence_subject.clone(),
                quad.predicate,
                quad.object,
                quad.graph_name,
            )
        })
        .collect();

    Ok(replaced_quads)
}

fn add_create_user_id_quad(
    quads: &mut Vec<Quad>,
    occurrence_uri: &str,
    create_user_id: Uuid,
) -> Result<(), OccurrenceServiceError> {
    let occurrence_subject = NamedNode::new(occurrence_uri)
        .map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

    let creator_predicate = NamedNode::new(CREATOR_PREDICATE_URI)
        .map_err(|_| OccurrenceServiceError::InvalidPredicateUri)?;

    let user_uri = format!("{}{}", USER_URI_BASE, create_user_id);

    let creator_resource = NamedNode::new(user_uri)
        .map_err(|_| OccurrenceServiceError::InvalidUserUri)?;

    let occurrence_graph = NamedNode::new(OCCURRENCE_GRAPH_URI)
        .map_err(|_| OccurrenceServiceError::InvalidGraphUri)?;

    let quad = Quad::new(
        occurrence_subject,
        creator_predicate,
        creator_resource,
        GraphName::NamedNode(occurrence_graph),
    );

    quads.push(quad);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replace_all_subjects_with_occurrence_uri_replaces_any_frontend_subject() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <https://evil.example/fake-occurrence> <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let replaced_quads = replace_all_subjects_with_occurrence_uri(
            quads,
            occurrence_uri,
        )
        .expect("all frontend subjects should be replaced");

        assert_eq!(replaced_quads.len(), 2);

        assert!(replaced_quads.iter().all(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
        }));

        assert!(replaced_quads.iter().all(|quad| {
            quad.graph_name.to_string()
                == "<https://bio-database.net/graphs/occurrences>"
        }));
    }

    #[test]
    fn add_create_user_id_quad_adds_creator_resource_in_occurrence_graph() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .expect("valid uuid");

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(
            quads,
            occurrence_uri,
        )
        .expect("all subjects should be replaced");

        add_create_user_id_quad(
            &mut quads,
            occurrence_uri,
            create_user_id,
        )
        .expect("create user id quad should be added");

        assert_eq!(quads.len(), 2);

        let has_creator_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string()
                    == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string()
                    == "<https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa>"
                && quad.graph_name.to_string()
                    == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "dcterms:creator quad should point to backend-confirmed user URI in occurrence graph"
        );
    }

    #[test]
    fn serialize_quads_as_nquads_outputs_named_graph_quads() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .expect("valid uuid");

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(
            quads,
            occurrence_uri,
        )
        .expect("all subjects should be replaced");

        add_create_user_id_quad(
            &mut quads,
            occurrence_uri,
            create_user_id,
        )
        .expect("creator quad should be added");

        let serialized = serialize_quads_as_nquads(&quads)
            .expect("quads should be serialized as n-quads");

        let serialized_text = String::from_utf8(serialized.clone())
            .expect("serialized n-quads should be utf-8");

        assert!(serialized_text.contains(
            "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000> <https://example.org/vocab/taxonName> \"Lumbricus terrestris\" <https://bio-database.net/graphs/occurrences> ."
        ));

        assert!(serialized_text.contains(
            "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa> <https://bio-database.net/graphs/occurrences> ."
        ));

        let parsed_again = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(&serialized)
            .collect::<Result<Vec<_>, _>>()
            .expect("serialized n-quads should be valid n-quads");

        assert_eq!(parsed_again.len(), 2);
    }
}