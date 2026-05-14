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
}

pub struct OccurrenceService;

impl OccurrenceService {
    pub async fn create_occurrence(
        _input: CreateOccurrenceInput,
    ) -> Result<CreateOccurrenceOutput, OccurrenceServiceError> {
        Err(OccurrenceServiceError::NotImplemented)
    }
}

const CREATOR_PREDICATE_URI: &str = "http://purl.org/dc/terms/creator"; //ダブリン・コアの作成者

const USER_URI_BASE: &str = "https://bio-database.net/users/"; //bio-database上のusers空間

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

    let quad = Quad::new(
        occurrence_subject,
        creator_predicate,
        creator_resource,
        GraphName::DefaultGraph,
    );

    quads.push(quad);

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_occurrence_returns_not_implemented_until_jena_client_is_added() {
        let input = CreateOccurrenceInput {
            create_user_id: Uuid::new_v4(),
            content_type: "text/turtle".to_string(),
            rdf_body: br#"
@prefix ex: <https://example.org/occurrence/> .

ex:occurrence-001
    a ex:Occurrence .
"#
            .to_vec(),
        };

        let result = OccurrenceService::create_occurrence(input).await;

        assert!(matches!(
            result,
            Err(OccurrenceServiceError::NotImplemented)
        ));
    }

    #[test]
    fn frontend_empty_subject_is_resolved_to_occurrence_uri_by_base_iri() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"@prefix ex: <https://example.org/vocab/> .

    <>
        ex:taxonName "Lumbricus terrestris" .
    "#;

        let occurrence_uri =
            "https://example.org/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let parser = RdfParser::from_format(RdfFormat::Turtle) //パーサー作成
            .with_base_iri(occurrence_uri)
            .expect("occurrence uri should be a valid base iri")
            .without_named_graphs();

        let quads = parser
            .for_slice(input) //一文ずつ
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend turtle should be parsed");

        assert_eq!(quads.len(), 1);
        assert_eq!(
            quads[0].subject.to_string(),
            "<https://example.org/occurrences/550e8400-e29b-41d4-a716-446655440000>"
        );
    }

    #[test]
    fn add_create_user_id_quad_adds_backend_user_as_uri_resource() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"@prefix ex: <https://example.org/vocab/> .

    <>
        ex:taxonName "Lumbricus terrestris" .
    "#;

        let occurrence_uri =
            "https://example.org/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .expect("valid uuid");

        let parser = RdfParser::from_format(RdfFormat::Turtle)
            .with_base_iri(occurrence_uri)
            .expect("occurrence uri should be a valid base iri")
            .without_named_graphs();

        let mut quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend turtle should be parsed");

        add_create_user_id_quad(
            &mut quads,
            occurrence_uri,
            create_user_id,
        )
        .expect("create user id quad should be added");

        assert_eq!(quads.len(), 2);

        let has_creator_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://example.org/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string()
                    == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string()
                    == "<https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa>"
        });

        assert!(
            has_creator_quad,
            "dcterms:creator quad should point to backend-confirmed user URI"
        );
    }
}