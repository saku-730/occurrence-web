use oxrdf::{
    GraphName,
    NamedNode,
    Quad,
};
use uuid::Uuid;
use oxrdfio::{
    RdfFormat,
    RdfSerializer,
    RdfParser,
};

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
    pub occurrence_id: Uuid,
    pub occurrence_uri: String,
    pub nquads: Vec<u8>,
}

#[derive(Debug)]
struct BuiltOccurrenceNquads {
    occurrence_id: Uuid,
    occurrence_uri: String,
    nquads: Vec<u8>,
}

pub struct GetOccurrenceInput {
    pub occurrence_id: Uuid,
}

pub struct GetOccurrenceOutput {
    pub nquads: Vec<u8>,
}

#[async_trait::async_trait]
pub trait OccurrenceRdfStore: Send + Sync { //rdfストアの粗結合実装。fakeでもfusekiでもどっちでも対応できるようにtrait
    async fn save_nquads(
        &self,
        nquads: Vec<u8>,
    ) -> Result<(), OccurrenceServiceError>;

    async fn get_occurrence_nquads(
        &self,
        occurrence_uri: &str,
    ) -> Result<Option<Vec<u8>>, OccurrenceServiceError>;
}

#[derive(Debug)]
pub enum OccurrenceServiceError {
    NotImplemented,
    InvalidOccurrenceUri,
    InvalidPredicateUri,
    InvalidUserUri,
    InvalidGraphUri,
    RdfSerializationFailed,
    RdfParseFailed,
    StoreFailed,
    FrontendManagedPredicateProvided,//フロントから誤ってユーザー情報が送られた場合。ユーザー偽装
    ForbiddenRdfGraph,//グラフの名前がoccurrence空間以外
    EmptyRdf, //空のデータ送信
}

const OCCURRENCE_URI_BASE: &str = "https://bio-database.net/occurrences/";
const CREATOR_PREDICATE_URI: &str = "http://purl.org/dc/terms/creator";
const USER_URI_BASE: &str = "https://bio-database.net/users/";
const OCCURRENCE_GRAPH_URI: &str = "https://bio-database.net/graphs/occurrences";

pub struct OccurrenceService;

impl OccurrenceService {
    pub async fn create_occurrence<S>(
        input: CreateOccurrenceInput,
        store: &S,
    ) -> Result<CreateOccurrenceOutput, OccurrenceServiceError>
    where
        S: OccurrenceRdfStore + ?Sized,
    {
        let output = Self::prepare_occurrence_for_storage(input)?;

        store.save_nquads(output.nquads.clone()).await?;

        Ok(output)
    }

    pub(crate) fn prepare_occurrence_for_storage(
        input: CreateOccurrenceInput,
    ) -> Result<CreateOccurrenceOutput, OccurrenceServiceError> {
        let built = build_occurrence_nquads_with_generated_id(
            &input.rdf_body,
            input.create_user_id,
        )?;

        Ok(CreateOccurrenceOutput {
            occurrence_id: built.occurrence_id,
            occurrence_uri: built.occurrence_uri,
            nquads: built.nquads,
        })
    }

    pub async fn get_occurrence<S>(
        input: GetOccurrenceInput,
        store: &S,
    ) -> Result<Option<GetOccurrenceOutput>, OccurrenceServiceError>
    where
        S: OccurrenceRdfStore + ?Sized,
    {
        let occurrence_uri = build_occurrence_uri(input.occurrence_id);

        let nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await?;

        Ok(nquads.map(|nquads| GetOccurrenceOutput { nquads }))
    }
}

fn replace_all_subjects_with_occurrence_uri( //主語にoccurrence uuidを追加
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

fn add_create_user_id_quad( //作成者情報を追加
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

fn serialize_quads_as_nquads( //再度シリアライズ
    quads: &[Quad],
) -> Result<Vec<u8>, OccurrenceServiceError> {
    let mut serializer =
        RdfSerializer::from_format(RdfFormat::NQuads).for_writer(Vec::new());

    for quad in quads {
        serializer
            .serialize_quad(quad)
            .map_err(|_| OccurrenceServiceError::RdfSerializationFailed)?;
    }

    serializer
        .finish()
        .map_err(|_| OccurrenceServiceError::RdfSerializationFailed)
}

fn build_occurrence_nquads( //フロントから来たN-Quadsを組み立て
    frontend_nquads: &[u8],
    occurrence_uri: &str,
    create_user_id: Uuid,
) -> Result<Vec<u8>, OccurrenceServiceError> {
    let quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(frontend_nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OccurrenceServiceError::RdfParseFailed)?;
    
    ensure_rdf_contains_at_least_one_quad(&quads)?;

    ensure_only_occurrence_graph(&quads)?;

    ensure_no_backend_managed_predicates(&quads)?;

    let mut quads = replace_all_subjects_with_occurrence_uri(
        quads,
        occurrence_uri,
    )?;

    add_create_user_id_quad(
        &mut quads,
        occurrence_uri,
        create_user_id,
    )?;

    serialize_quads_as_nquads(&quads)
}

fn build_occurrence_nquads_with_generated_id(
    frontend_nquads: &[u8],
    create_user_id: Uuid,
) -> Result<BuiltOccurrenceNquads, OccurrenceServiceError> {
    let occurrence_id = Uuid::new_v4();
    let occurrence_uri = format!("{}{}", OCCURRENCE_URI_BASE, occurrence_id);

    let nquads = build_occurrence_nquads(
        frontend_nquads,
        &occurrence_uri,
        create_user_id,
    )?;

    Ok(BuiltOccurrenceNquads {
        occurrence_id,
        occurrence_uri,
        nquads,
    })
}

fn ensure_no_backend_managed_predicates( //ユーザー情報がフロントから送られていないことを確認。
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let has_backend_managed_predicate = quads
        .iter()
        .any(|quad| quad.predicate.as_str() == CREATOR_PREDICATE_URI);

    if has_backend_managed_predicate {
        return Err(OccurrenceServiceError::FrontendManagedPredicateProvided);
    }

    Ok(())
}

fn ensure_only_occurrence_graph( //グラフ名が間違っていれば拒否
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let all_quads_use_occurrence_graph = quads.iter().all(|quad| {
        match &quad.graph_name {
            GraphName::NamedNode(graph_name) => {
                graph_name.as_str() == OCCURRENCE_GRAPH_URI
            }
            GraphName::DefaultGraph => false,
            GraphName::BlankNode(_) => false,
        }
    });

    if !all_quads_use_occurrence_graph {
        return Err(OccurrenceServiceError::ForbiddenRdfGraph);
    }

    Ok(())
}

fn ensure_rdf_contains_at_least_one_quad( //空のデータを拒否
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    if quads.is_empty() {
        return Err(OccurrenceServiceError::EmptyRdf);
    }

    Ok(())
}

fn build_occurrence_uri(occurrence_id: Uuid) -> String {
    format!(
        "https://bio-database.net/occurrences/{}",
        occurrence_id
    )
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

    #[test]
    fn build_occurrence_nquads_replaces_subject_and_adds_creator() {
        use oxrdfio::{RdfFormat, RdfParser};

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <https://evil.example/fake-occurrence> <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .expect("valid uuid");

        let built = build_occurrence_nquads(
            frontend_nquads,
            occurrence_uri,
            create_user_id,
        )
        .expect("occurrence n-quads should be built");

        let built_text = String::from_utf8(built.clone())
            .expect("built n-quads should be utf-8");

        assert!(built_text.contains(
            "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000> <https://example.org/vocab/taxonName> \"Lumbricus terrestris\" <https://bio-database.net/graphs/occurrences> ."
        ));

        assert!(built_text.contains(
            "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000> <https://example.org/vocab/locality> \"somewhere\" <https://bio-database.net/graphs/occurrences> ."
        ));

        assert!(built_text.contains(
            "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa> <https://bio-database.net/graphs/occurrences> ."
        ));

        let parsed_again = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(&built)
            .collect::<Result<Vec<_>, _>>()
            .expect("built output should be valid n-quads");

        assert_eq!(parsed_again.len(), 3);

        assert!(parsed_again.iter().all(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
        }));
    }

    #[test]
    fn prepare_occurrence_for_storage_generates_id_and_builds_nquads() {
        use oxrdfio::{RdfFormat, RdfParser};

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <https://evil.example/fake-occurrence> <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .expect("valid uuid");

        let input = CreateOccurrenceInput {
            create_user_id,
            content_type: "application/n-quads".to_string(),
            rdf_body: frontend_nquads.to_vec(),
        };

        let output = OccurrenceService::prepare_occurrence_for_storage(input)
            .expect("occurrence should be prepared for storage");

        assert!(
            output.occurrence_uri.starts_with(OCCURRENCE_URI_BASE),
            "occurrence URI should start with occurrence URI base"
        );

        let occurrence_id_in_uri = output
            .occurrence_uri
            .strip_prefix(OCCURRENCE_URI_BASE)
            .expect("occurrence URI should contain UUID suffix");

        let parsed_occurrence_id =
            uuid::Uuid::parse_str(occurrence_id_in_uri)
                .expect("occurrence URI suffix should be UUID");

        assert_eq!(output.occurrence_id, parsed_occurrence_id);

        let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(&output.nquads)
            .collect::<Result<Vec<_>, _>>()
            .expect("output n-quads should be valid");

        assert_eq!(parsed_quads.len(), 3);

        let expected_subject = format!("<{}>", output.occurrence_uri);

        assert!(
            parsed_quads.iter().all(|quad| {
                quad.subject.to_string() == expected_subject
            }),
            "all subjects should be backend-issued occurrence URI"
        );

        let expected_creator_object = format!(
            "<https://bio-database.net/users/{}>",
            create_user_id
        );

        let has_creator_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string() == expected_creator_object
                && quad.graph_name.to_string()
                    == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "output should contain dcterms:creator quad"
        );
    }
    
    #[tokio::test]
    async fn create_occurrence_saves_built_nquads_to_store() {
        use std::sync::{Arc, Mutex};
        use oxrdfio::{RdfFormat, RdfParser};

        #[derive(Clone, Default)]
        struct FakeOccurrenceRdfStore {
            saved_nquads: Arc<Mutex<Vec<Vec<u8>>>>,
        }

        #[async_trait::async_trait]
        impl OccurrenceRdfStore for FakeOccurrenceRdfStore {
            async fn save_nquads(
                &self,
                nquads: Vec<u8>,
            ) -> Result<(), OccurrenceServiceError> {
                self.saved_nquads
                    .lock()
                    .expect("mutex should not be poisoned")
                    .push(nquads);

                Ok(())
            }

            async fn get_occurrence_nquads(
                &self,
                _occurrence_uri: &str,
            ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
                Ok(None)
            }
        }

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                .expect("valid uuid");

        let input = CreateOccurrenceInput {
            create_user_id,
            content_type: "application/n-quads".to_string(),
            rdf_body: frontend_nquads.to_vec(),
        };

        let store = FakeOccurrenceRdfStore::default();

        let output = OccurrenceService::create_occurrence(input, &store)
            .await
            .expect("occurrence should be created");

        assert!(
            output.occurrence_uri.starts_with(OCCURRENCE_URI_BASE)
        );

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(saved.len(), 1);

        assert_eq!(saved[0], output.nquads);

        let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(&saved[0])
            .collect::<Result<Vec<_>, _>>()
            .expect("saved n-quads should be valid");

        assert_eq!(parsed_quads.len(), 2);

        let expected_subject = format!("<{}>", output.occurrence_uri);

        assert!(
            parsed_quads.iter().all(|quad| {
                quad.subject.to_string() == expected_subject
            }),
            "all saved quads should use backend-issued occurrence URI"
        );

        let has_creator_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string()
                    == "<https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa>"
                && quad.graph_name.to_string()
                    == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "saved n-quads should contain dcterms:creator quad"
        );
    }
}