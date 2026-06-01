use chrono::{DateTime, SecondsFormat, Utc};
use oxrdf::{GraphName, Literal, NamedNode, NamedOrBlankNode, Quad, Term, vocab::xsd};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};
use uuid::Uuid;

#[derive(Debug)]
pub struct CreateOccurrenceInput {
    //occurrenceデータ作成時の構造体
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
pub trait OccurrenceRdfStore: Send + Sync {
    //rdfストアの粗結合実装。fakeでもfusekiでもどっちでも対応できるようにtrait
    async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError>;

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
    FrontendManagedPredicateProvided, //フロントから誤ってユーザー情報が送られた場合。ユーザー偽装
    ForbiddenRdfGraph,                //グラフの名前がoccurrence空間以外
    EmptyRdf,                         //空のデータ送信
    InvalidAccessRights,               //accessRightsの値が仕様外
    InvalidLicense,                    //licenseの値が仕様外
    InvalidBlankNodeSubject,            //blank node subjectが仕様外
    InvalidObjectBlankNode,              //object blank nodeは拒否
}

const OCCURRENCE_URI_BASE: &str = "https://bio-database.net/occurrences/";
const CREATOR_PREDICATE_URI: &str = "http://purl.org/dc/terms/creator";
const CREATED_PREDICATE_URI: &str = "http://purl.org/dc/terms/created";
const MODIFIED_PREDICATE_URI: &str = "http://purl.org/dc/terms/modified";
const ACCESS_RIGHTS_PREDICATE_URI: &str = "http://purl.org/dc/terms/accessRights";
const LICENSE_PREDICATE_URI: &str = "http://purl.org/dc/terms/license";
const CREATIVE_COMMONS_LICENSE_URI_PREFIX: &str = "https://creativecommons.org/";
const PUBLIC_ACCESS_RIGHTS_URI: &str = "https://bio-database.net/terms/access-rights/public";
const PRIVATE_ACCESS_RIGHTS_URI: &str = "https://bio-database.net/terms/access-rights/private";
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
        let built =
            build_occurrence_nquads_with_generated_id(&input.rdf_body, input.create_user_id)?;

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

        let nquads = store.get_occurrence_nquads(&occurrence_uri).await?;

        Ok(nquads.map(|nquads| GetOccurrenceOutput { nquads }))
    }
}

fn replace_all_subjects_with_occurrence_uri(
    //主語にoccurrence uuidを追加
    quads: Vec<Quad>,
    occurrence_uri: &str,
) -> Result<Vec<Quad>, OccurrenceServiceError> {
    let occurrence_subject =
        NamedNode::new(occurrence_uri).map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

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
    //作成者情報を追加
    quads: &mut Vec<Quad>,
    occurrence_uri: &str,
    create_user_id: Uuid,
) -> Result<(), OccurrenceServiceError> {
    let occurrence_subject =
        NamedNode::new(occurrence_uri).map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

    let creator_predicate = NamedNode::new(CREATOR_PREDICATE_URI)
        .map_err(|_| OccurrenceServiceError::InvalidPredicateUri)?;

    let user_uri = format!("{}{}", USER_URI_BASE, create_user_id);

    let creator_resource =
        NamedNode::new(user_uri).map_err(|_| OccurrenceServiceError::InvalidUserUri)?;

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

fn add_created_quad(
    quads: &mut Vec<Quad>,
    occurrence_uri: &str,
    created_at: DateTime<Utc>,
) -> Result<(), OccurrenceServiceError> {
    let occurrence_subject =
        NamedNode::new(occurrence_uri).map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

    let created_predicate = NamedNode::new(CREATED_PREDICATE_URI)
        .map_err(|_| OccurrenceServiceError::InvalidPredicateUri)?;

    let occurrence_graph = NamedNode::new(OCCURRENCE_GRAPH_URI)
        .map_err(|_| OccurrenceServiceError::InvalidGraphUri)?;

    let created_literal = Literal::new_typed_literal(
        created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        xsd::DATE_TIME,
    );

    let quad = Quad::new(
        occurrence_subject,
        created_predicate,
        created_literal,
        GraphName::NamedNode(occurrence_graph),
    );

    quads.push(quad);

    Ok(())
}

fn add_modified_quad(
    quads: &mut Vec<Quad>,
    occurrence_uri: &str,
    modified_at: DateTime<Utc>,
) -> Result<(), OccurrenceServiceError> {
    let occurrence_subject =
        NamedNode::new(occurrence_uri).map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

    let modified_predicate = NamedNode::new(MODIFIED_PREDICATE_URI)
        .map_err(|_| OccurrenceServiceError::InvalidPredicateUri)?;

    let occurrence_graph = NamedNode::new(OCCURRENCE_GRAPH_URI)
        .map_err(|_| OccurrenceServiceError::InvalidGraphUri)?;

    // 作成時点の更新日時を、RDF仕様側で検証しやすいxsd:dateTimeとして保存する。
    let modified_literal = Literal::new_typed_literal(
        modified_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        xsd::DATE_TIME,
    );

    let quad = Quad::new(
        occurrence_subject,
        modified_predicate,
        modified_literal,
        GraphName::NamedNode(occurrence_graph),
    );

    quads.push(quad);

    Ok(())
}

fn add_default_access_rights_quad_if_missing(
    quads: &mut Vec<Quad>,
    occurrence_uri: &str,
) -> Result<(), OccurrenceServiceError> {
    let already_has_access_rights = quads
        .iter()
        .any(|quad| quad.predicate.as_str() == ACCESS_RIGHTS_PREDICATE_URI);

    if already_has_access_rights {//フロントからアクセス権限情報が送られていればなにもしない。
        return Ok(());
    }

    let occurrence_subject =
        NamedNode::new(occurrence_uri).map_err(|_| OccurrenceServiceError::InvalidOccurrenceUri)?;

    let access_rights_predicate = NamedNode::new(ACCESS_RIGHTS_PREDICATE_URI)
        .map_err(|_| OccurrenceServiceError::InvalidPredicateUri)?;

    let public_access_rights = NamedNode::new(PUBLIC_ACCESS_RIGHTS_URI)
        .map_err(|_| OccurrenceServiceError::InvalidPredicateUri)?;

    let occurrence_graph = NamedNode::new(OCCURRENCE_GRAPH_URI)
        .map_err(|_| OccurrenceServiceError::InvalidGraphUri)?;

    // accessRights未指定時は、MVP仕様に従ってpublicをbackend側で明示する。
    let quad = Quad::new(
        occurrence_subject,
        access_rights_predicate,
        public_access_rights,
        GraphName::NamedNode(occurrence_graph),
    );

    quads.push(quad);

    Ok(())
}

fn ensure_access_rights_is_resource(
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let access_rights_quads = quads
        .iter()
        .filter(|quad| quad.predicate.as_str() == ACCESS_RIGHTS_PREDICATE_URI)
        .collect::<Vec<_>>();

    // accessRightsは1つだけ許可する。複数あると公開範囲を一意に決められない。
    if access_rights_quads.len() > 1 {
        return Err(OccurrenceServiceError::InvalidAccessRights);
    }

    for quad in access_rights_quads {
        let access_rights_uri = match &quad.object {
            // accessRightsがURIであることを確認する。
            Term::NamedNode(access_rights_uri) => access_rights_uri.as_str(),
            _ => return Err(OccurrenceServiceError::InvalidAccessRights),
        };

        // accessRightsはMVP仕様で定義したpublic/private URIだけ許可する。
        if access_rights_uri != PUBLIC_ACCESS_RIGHTS_URI
            && access_rights_uri != PRIVATE_ACCESS_RIGHTS_URI
        {
            return Err(OccurrenceServiceError::InvalidAccessRights);
        }
    }

    Ok(())
}

fn ensure_license_is_creative_commons_resource(
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let license_quads = quads
        .iter()
        .filter(|quad| quad.predicate.as_str() == LICENSE_PREDICATE_URI)
        .collect::<Vec<_>>();

    // licenseは1つだけ許可する。複数あると適用される利用条件が曖昧になる。
    if license_quads.len() > 1 {
        return Err(OccurrenceServiceError::InvalidLicense);
    }

    for quad in license_quads {
        let license_uri = match &quad.object {
            // licenseは機械的に判定できるCreative Commons URIだけを許可する。
            Term::NamedNode(license_uri) => license_uri.as_str(),
            _ => return Err(OccurrenceServiceError::InvalidLicense),
        };

        if !license_uri.starts_with(CREATIVE_COMMONS_LICENSE_URI_PREFIX) {
            return Err(OccurrenceServiceError::InvalidLicense);
        }
    }

    Ok(())
}

fn serialize_quads_as_nquads(
    //再度シリアライズ
    quads: &[Quad],
) -> Result<Vec<u8>, OccurrenceServiceError> {
    let mut serializer = RdfSerializer::from_format(RdfFormat::NQuads).for_writer(Vec::new());

    for quad in quads {
        serializer
            .serialize_quad(quad)
            .map_err(|_| OccurrenceServiceError::RdfSerializationFailed)?;
    }

    serializer
        .finish()
        .map_err(|_| OccurrenceServiceError::RdfSerializationFailed)
}

fn build_occurrence_nquads(
    //フロントから来たN-Quadsを組み立て
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

    ensure_single_blank_node_subject(&quads)?;

    ensure_no_object_blank_node(&quads)?;

    ensure_no_backend_managed_predicates(&quads)?;

    ensure_access_rights_is_resource(&quads)?;

    ensure_license_is_creative_commons_resource(&quads)?;

    let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)?;

    add_create_user_id_quad(&mut quads, occurrence_uri, create_user_id)?;

    // 作成直後のcreatedとmodifiedは仕様上同じ時刻にする。
    let now = Utc::now();
    add_created_quad(&mut quads, occurrence_uri, now)?;
    add_modified_quad(&mut quads, occurrence_uri, now)?;
    add_default_access_rights_quad_if_missing(&mut quads, occurrence_uri)?;

    serialize_quads_as_nquads(&quads)
}

fn build_occurrence_nquads_with_generated_id(
    frontend_nquads: &[u8],
    create_user_id: Uuid,
) -> Result<BuiltOccurrenceNquads, OccurrenceServiceError> {
    let occurrence_id = Uuid::new_v4();
    let occurrence_uri = format!("{}{}", OCCURRENCE_URI_BASE, occurrence_id);

    let nquads = build_occurrence_nquads(frontend_nquads, &occurrence_uri, create_user_id)?;

    Ok(BuiltOccurrenceNquads {
        occurrence_id,
        occurrence_uri,
        nquads,
    })
}

fn ensure_single_blank_node_subject(
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let mut blank_node_subjects = Vec::new();

    for quad in quads {
        let NamedOrBlankNode::BlankNode(blank_node) = &quad.subject else {
            // frontendは保存前の仮subjectとしてblank nodeだけを送る。URI subjectは偽装防止のため拒否する。
            return Err(OccurrenceServiceError::InvalidBlankNodeSubject);
        };

        let blank_node_id = blank_node.as_str();

        if !blank_node_subjects.contains(&blank_node_id) {
            blank_node_subjects.push(blank_node_id);
        }
    }

    // 1リクエストで作成できるoccurrenceは1件だけなので、blank node subjectも1つだけ許可する。
    if blank_node_subjects.len() != 1 {
        return Err(OccurrenceServiceError::InvalidBlankNodeSubject);
    }

    Ok(())
}

fn ensure_no_object_blank_node(
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let has_object_blank_node = quads
        .iter()
        .any(|quad| matches!(quad.object, Term::BlankNode(_)));

    // object blank node は保存後に参照範囲が曖昧になりやすいためMVPでは拒否する。
    if has_object_blank_node {
        return Err(OccurrenceServiceError::InvalidObjectBlankNode);
    }

    Ok(())
}

fn ensure_no_backend_managed_predicates(
    // creator / created / modified はbackendが確定するため、フロントからの送信を拒否する。
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let has_backend_managed_predicate = quads.iter().any(|quad| {
        let predicate = quad.predicate.as_str();

        predicate == CREATOR_PREDICATE_URI
            || predicate == CREATED_PREDICATE_URI
            || predicate == MODIFIED_PREDICATE_URI
    });

    if has_backend_managed_predicate {
        return Err(OccurrenceServiceError::FrontendManagedPredicateProvided);
    }

    Ok(())
}

fn ensure_only_occurrence_graph(
    //グラフ名が間違っていれば拒否
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    let all_quads_use_occurrence_graph = quads.iter().all(|quad| match &quad.graph_name {
        GraphName::NamedNode(graph_name) => graph_name.as_str() == OCCURRENCE_GRAPH_URI,
        GraphName::DefaultGraph => false,
        GraphName::BlankNode(_) => false,
    });

    if !all_quads_use_occurrence_graph {
        return Err(OccurrenceServiceError::ForbiddenRdfGraph);
    }

    Ok(())
}

fn ensure_rdf_contains_at_least_one_quad(
    //空のデータを拒否
    quads: &[Quad],
) -> Result<(), OccurrenceServiceError> {
    if quads.is_empty() {
        return Err(OccurrenceServiceError::EmptyRdf);
    }

    Ok(())
}

fn build_occurrence_uri(occurrence_id: Uuid) -> String {
    format!("https://bio-database.net/occurrences/{}", occurrence_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replace_all_subjects_with_occurrence_uri_replaces_blank_node_subjects() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    _:occurrence <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let replaced_quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all frontend subjects should be replaced");

        assert_eq!(replaced_quads.len(), 2);

        assert!(replaced_quads.iter().all(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
        }));

        assert!(replaced_quads.iter().all(|quad| {
            quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
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
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all subjects should be replaced");

        add_create_user_id_quad(&mut quads, occurrence_uri, create_user_id)
            .expect("create user id quad should be added");

        assert_eq!(quads.len(), 2);

        let has_creator_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string()
                    == "<https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "dcterms:creator quad should point to backend-confirmed user URI in occurrence graph"
        );
    }

    #[test]
    fn add_created_quad_adds_created_datetime_in_occurrence_graph() {
        use chrono::{TimeZone, Utc};
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let created_at = Utc
            .with_ymd_and_hms(2026, 5, 29, 12, 34, 56)
            .single()
            .expect("valid UTC datetime");

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all subjects should be replaced");

        add_created_quad(&mut quads, occurrence_uri, created_at)
            .expect("created quad should be added");

        assert_eq!(quads.len(), 2);

        let has_created_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string() == "<http://purl.org/dc/terms/created>"
                && quad.object.to_string()
                    == "\"2026-05-29T12:34:56Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_created_quad,
            "dcterms:created quad should use backend current time as xsd:dateTime in occurrence graph"
        );
    }

    #[test]
    fn add_modified_quad_adds_modified_datetime_in_occurrence_graph() {
        use chrono::{TimeZone, Utc};
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let modified_at = Utc
            .with_ymd_and_hms(2026, 5, 29, 12, 34, 56)
            .single()
            .expect("valid UTC datetime");

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all subjects should be replaced");

        add_modified_quad(&mut quads, occurrence_uri, modified_at)
            .expect("modified quad should be added");

        assert_eq!(quads.len(), 2);

        let has_modified_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string() == "<http://purl.org/dc/terms/modified>"
                && quad.object.to_string()
                    == "\"2026-05-29T12:34:56Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_modified_quad,
            "dcterms:modified quad should use backend current time as xsd:dateTime in occurrence graph"
        );
    }

    #[test]
    fn add_default_access_rights_quad_if_missing_adds_public_access_rights() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all subjects should be replaced");

        add_default_access_rights_quad_if_missing(&mut quads, occurrence_uri)
            .expect("default access rights quad should be added");

        assert_eq!(quads.len(), 2);

        let has_access_rights_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string() == "<http://purl.org/dc/terms/accessRights>"
                && quad.object.to_string()
                    == "<https://bio-database.net/terms/access-rights/public>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_access_rights_quad,
            "missing dcterms:accessRights should default to public URI in occurrence graph"
        );
    }

    #[test]
    fn add_default_access_rights_quad_if_missing_keeps_frontend_access_rights() {
        use oxrdfio::{RdfFormat, RdfParser};

        let input = br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all subjects should be replaced");

        add_default_access_rights_quad_if_missing(&mut quads, occurrence_uri)
            .expect("existing access rights should be kept");

        assert_eq!(
            quads.len(),
            1,
            "accessRights already sent by frontend should not be duplicated"
        );

        let has_private_access_rights_quad = quads.iter().any(|quad| {
            quad.subject.to_string()
                == "<https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000>"
                && quad.predicate.to_string() == "<http://purl.org/dc/terms/accessRights>"
                && quad.object.to_string()
                    == "<https://bio-database.net/terms/access-rights/private>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_private_access_rights_quad,
            "frontend-provided dcterms:accessRights should be kept as-is"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_literal_access_rights() {
        let frontend_nquads = br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> "public" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidAccessRights)),
            "literal dcterms:accessRights should be rejected"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_unknown_access_rights_uri() {
        let frontend_nquads = br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://example.org/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidAccessRights)),
            "unknown dcterms:accessRights URI should be rejected"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_multiple_access_rights() {
        let frontend_nquads = br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidAccessRights)),
            "multiple dcterms:accessRights values should be rejected"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_non_creative_commons_license_uri() {
        let frontend_nquads = br#"
    _:occurrence <http://purl.org/dc/terms/license> <https://example.org/license/custom> <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidLicense)),
            "license URI outside Creative Commons should be rejected"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_frontend_backend_managed_predicates() {
        let cases = [
            (
                "creator",
                br#"
    _:occurrence <http://purl.org/dc/terms/creator> <https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
            (
                "created",
                br#"
    _:occurrence <http://purl.org/dc/terms/created> "2026-05-29T12:34:56Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
            (
                "modified",
                br#"
    _:occurrence <http://purl.org/dc/terms/modified> "2026-05-29T12:34:56Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
        ];

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        for (predicate_name, frontend_nquads) in cases {
            let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

            assert!(
                matches!(
                    result,
                    Err(OccurrenceServiceError::FrontendManagedPredicateProvided)
                ),
                "frontend-sent dcterms:{predicate_name} should be rejected"
            );
        }
    }

    #[test]
    fn build_occurrence_nquads_rejects_named_node_subject() {
        let frontend_nquads = br#"
    <https://evil.example/fake-occurrence> <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidBlankNodeSubject)),
            "frontend subject should be the single blank node for the occurrence"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_multiple_blank_node_subjects() {
        let frontend_nquads = br#"
    _:occurrence_a <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    _:occurrence_b <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidBlankNodeSubject)),
            "multiple blank node subjects should be rejected"
        );
    }

    #[test]
    fn build_occurrence_nquads_rejects_object_blank_node() {
        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/relatedObject> _:object <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let result = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id);

        assert!(
            matches!(result, Err(OccurrenceServiceError::InvalidObjectBlankNode)),
            "object blank node should be rejected"
        );
    }

    #[test]
    fn build_occurrence_nquads_keeps_valid_access_rights_values() {
        use oxrdfio::{RdfFormat, RdfParser};

        let cases = [
            (
                "public",
                "https://bio-database.net/terms/access-rights/public",
            ),
            (
                "private",
                "https://bio-database.net/terms/access-rights/private",
            ),
        ];

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        for (label, access_rights_uri) in cases {
            let frontend_nquads = format!(
                "_:occurrence <http://purl.org/dc/terms/accessRights> <{}> <https://bio-database.net/graphs/occurrences> .\n",
                access_rights_uri
            );

            let built = build_occurrence_nquads(
                frontend_nquads.as_bytes(),
                occurrence_uri,
                create_user_id,
            )
            .expect("valid accessRights should be accepted");

            let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
                .for_slice(&built)
                .collect::<Result<Vec<_>, _>>()
                .expect("built n-quads should be valid");

            let access_rights_quads = parsed_quads
                .iter()
                .filter(|quad| {
                    quad.predicate.to_string() == "<http://purl.org/dc/terms/accessRights>"
                })
                .collect::<Vec<_>>();

            assert_eq!(
                access_rights_quads.len(),
                1,
                "valid {label} accessRights should not be duplicated"
            );

            assert_eq!(
                access_rights_quads[0].object.to_string(),
                format!("<{}>", access_rights_uri),
                "valid {label} accessRights should be kept"
            );
        }
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
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let parser = RdfParser::from_format(RdfFormat::NQuads);

        let quads = parser
            .for_slice(input)
            .collect::<Result<Vec<_>, _>>()
            .expect("frontend n-quads should be parsed");

        let mut quads = replace_all_subjects_with_occurrence_uri(quads, occurrence_uri)
            .expect("all subjects should be replaced");

        add_create_user_id_quad(&mut quads, occurrence_uri, create_user_id)
            .expect("creator quad should be added");

        let serialized =
            serialize_quads_as_nquads(&quads).expect("quads should be serialized as n-quads");

        let serialized_text =
            String::from_utf8(serialized.clone()).expect("serialized n-quads should be utf-8");

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
    _:occurrence <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let occurrence_uri =
            "https://bio-database.net/occurrences/550e8400-e29b-41d4-a716-446655440000";

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let built = build_occurrence_nquads(frontend_nquads, occurrence_uri, create_user_id)
            .expect("occurrence n-quads should be built");

        let built_text = String::from_utf8(built.clone()).expect("built n-quads should be utf-8");

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

        assert_eq!(parsed_again.len(), 6);

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
    _:occurrence <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "#;

        let create_user_id =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

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

        let parsed_occurrence_id = uuid::Uuid::parse_str(occurrence_id_in_uri)
            .expect("occurrence URI suffix should be UUID");

        assert_eq!(output.occurrence_id, parsed_occurrence_id);

        let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(&output.nquads)
            .collect::<Result<Vec<_>, _>>()
            .expect("output n-quads should be valid");

        assert_eq!(parsed_quads.len(), 6);

        let expected_subject = format!("<{}>", output.occurrence_uri);

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.subject.to_string() == expected_subject }),
            "all subjects should be backend-issued occurrence URI"
        );

        let expected_creator_object =
            format!("<https://bio-database.net/users/{}>", create_user_id);

        let has_creator_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string() == expected_creator_object
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "output should contain dcterms:creator quad"
        );
    }

    #[tokio::test]
    async fn get_occurrence_returns_nquads_for_requested_occurrence_uri() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct FakeOccurrenceRdfStore {
            expected_occurrence_uri: String,
            nquads: Vec<u8>,
            requested_occurrence_uris: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl OccurrenceRdfStore for FakeOccurrenceRdfStore {
            async fn save_nquads(&self, _nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
                Ok(())
            }

            async fn get_occurrence_nquads(
                &self,
                occurrence_uri: &str,
            ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
                self.requested_occurrence_uris
                    .lock()
                    .expect("mutex should not be poisoned")
                    .push(occurrence_uri.to_string());

                if occurrence_uri == self.expected_occurrence_uri {
                    Ok(Some(self.nquads.clone()))
                } else {
                    Ok(None)
                }
            }
        }

        let occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let expected_nquads = format!(
            "<{}> <https://example.org/vocab/scientificName> \"Lumbricus terrestris\" <https://bio-database.net/graphs/occurrences> .\n",
            occurrence_uri
        )
        .into_bytes();

        let requested_occurrence_uris = Arc::new(Mutex::new(Vec::new()));
        let store = FakeOccurrenceRdfStore {
            expected_occurrence_uri: occurrence_uri.clone(),
            nquads: expected_nquads.clone(),
            requested_occurrence_uris: requested_occurrence_uris.clone(),
        };

        let output = OccurrenceService::get_occurrence(GetOccurrenceInput { occurrence_id }, &store)
            .await
            .expect("get occurrence should succeed")
            .expect("requested occurrence should exist");

        assert_eq!(output.nquads, expected_nquads);

        let requested = requested_occurrence_uris
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(requested.as_slice(), &[occurrence_uri]);
    }

    #[tokio::test]
    async fn get_occurrence_returns_none_when_store_returns_none() {
        struct EmptyOccurrenceRdfStore;

        #[async_trait::async_trait]
        impl OccurrenceRdfStore for EmptyOccurrenceRdfStore {
            async fn save_nquads(&self, _nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
                Ok(())
            }

            async fn get_occurrence_nquads(
                &self,
                _occurrence_uri: &str,
            ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
                Ok(None)
            }
        }

        let occurrence_id = uuid::Uuid::new_v4();
        let store = EmptyOccurrenceRdfStore;

        let output = OccurrenceService::get_occurrence(GetOccurrenceInput { occurrence_id }, &store)
            .await
            .expect("get occurrence should succeed even when occurrence is missing");

        assert!(output.is_none());
    }

    #[tokio::test]
    async fn get_occurrence_propagates_store_failed_error() {
        struct FailingOccurrenceRdfStore;

        #[async_trait::async_trait]
        impl OccurrenceRdfStore for FailingOccurrenceRdfStore {
            async fn save_nquads(&self, _nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
                Ok(())
            }

            async fn get_occurrence_nquads(
                &self,
                _occurrence_uri: &str,
            ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
                Err(OccurrenceServiceError::StoreFailed)
            }
        }

        let occurrence_id = uuid::Uuid::new_v4();
        let store = FailingOccurrenceRdfStore;

        let result = OccurrenceService::get_occurrence(GetOccurrenceInput { occurrence_id }, &store)
            .await;

        assert!(
            matches!(result, Err(OccurrenceServiceError::StoreFailed)),
            "store failure should be propagated from get_occurrence"
        );
    }

    #[tokio::test]
    async fn create_occurrence_saves_built_nquads_to_store() {
        use oxrdfio::{RdfFormat, RdfParser};
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct FakeOccurrenceRdfStore {
            saved_nquads: Arc<Mutex<Vec<Vec<u8>>>>,
        }

        #[async_trait::async_trait]
        impl OccurrenceRdfStore for FakeOccurrenceRdfStore {
            async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
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
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("valid uuid");

        let input = CreateOccurrenceInput {
            create_user_id,
            content_type: "application/n-quads".to_string(),
            rdf_body: frontend_nquads.to_vec(),
        };

        let store = FakeOccurrenceRdfStore::default();

        let output = OccurrenceService::create_occurrence(input, &store)
            .await
            .expect("occurrence should be created");

        assert!(output.occurrence_uri.starts_with(OCCURRENCE_URI_BASE));

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

        assert_eq!(parsed_quads.len(), 5);

        let expected_subject = format!("<{}>", output.occurrence_uri);

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.subject.to_string() == expected_subject }),
            "all saved quads should use backend-issued occurrence URI"
        );

        let has_creator_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string()
                    == "<https://bio-database.net/users/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "saved n-quads should contain dcterms:creator quad"
        );
    }
}
