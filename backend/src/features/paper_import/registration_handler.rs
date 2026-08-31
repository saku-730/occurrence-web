use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::{CONTENT_TYPE, COOKIE}},
    response::{IntoResponse, Response},
};
use oxrdf::{GraphName, Literal, NamedNode, Quad};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};
use uuid::Uuid;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        occurrences::{
            dto::CreateOccurrenceResponse,
            service::{CreateOccurrenceInput, OccurrenceService, OccurrenceServiceError},
        },
    },
    state::AppState,
};

use super::repository::PaperRepository;

const OCCURRENCE_GRAPH_URI: &str = "https://bio-database.net/graphs/occurrences";
const ASSOCIATED_REFERENCES_PREDICATE_URI: &str =
    "http://rs.tdwg.org/dwc/terms/associatedReferences";
const SOURCE_PAPER_PREDICATE_URI: &str = "https://bio-database.net/terms/sourcePaper";
const PAPER_URI_BASE: &str = "https://bio-database.net/papers/";

#[derive(Debug)]
pub enum PaperRegistrationError {
    InvalidSession,
    InvalidInput,
    NotFound,
    InvalidRdf,
    StoreFailed,
    Database(sqlx::Error),
    Internal,
}

impl From<AuthServiceError> for PaperRegistrationError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<sqlx::Error> for PaperRegistrationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for PaperRegistrationError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                "invalid_session",
                "Invalid session",
            ),
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                "invalid_paper_occurrence",
                "Invalid paper occurrence request",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "paper_not_found",
                "Paper not found",
            ),
            Self::InvalidRdf => (
                StatusCode::BAD_REQUEST,
                "invalid_rdf",
                "Invalid occurrence RDF body",
            ),
            Self::StoreFailed => (
                StatusCode::BAD_GATEWAY,
                "rdf_store_error",
                "Failed to save occurrence RDF",
            ),
            Self::Database(_) | Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_server_error",
                "Internal server error",
            ),
        };

        (
            status,
            Json(ErrorResponse {
                error: error.to_string(),
                message: message.to_string(),
            }),
        )
            .into_response()
    }
}

/// Register one user-confirmed occurrence extracted from a paper.
///
/// The frontend sends exactly the same N-Quads body as the normal occurrence
/// registration screen. This endpoint adds paper provenance on the server and
/// then delegates persistence to the ordinary OccurrenceService.
pub async fn register_occurrence(
    State(state): State<AppState>,
    Path(paper_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<CreateOccurrenceResponse>), PaperRegistrationError> {
    ensure_nquads_content_type(&headers)?;
    if body.is_empty() {
        return Err(PaperRegistrationError::InvalidInput);
    }

    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let paper = PaperRepository::find_by_id(&state.posgre, paper_id)
        .await?
        .ok_or(PaperRegistrationError::NotFound)?;

    let associated_reference = paper_associated_reference(
        paper.doi.as_deref(),
        paper.title.as_deref(),
    )
    .ok_or(PaperRegistrationError::InvalidInput)?;

    let rdf_body = add_paper_provenance(&body, paper_id, &associated_reference)?;

    let output = OccurrenceService::create_occurrence(
        CreateOccurrenceInput {
            create_user_id: current_user.user_id,
            content_type: "application/n-quads".to_string(),
            rdf_body,
        },
        state.occurrence_rdf_store.as_ref(),
    )
    .await
    .map_err(map_occurrence_error)?;

    // `registered` means at least one occurrence from this paper has actually
    // reached Fuseki. Never set it before OccurrenceService returns success.
    if !PaperRepository::mark_registered(&state.posgre, paper_id).await? {
        return Err(PaperRegistrationError::NotFound);
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateOccurrenceResponse {
            occurrence_id: output.occurrence_id.to_string(),
            occurrence_uri: output.occurrence_uri,
        }),
    ))
}

fn ensure_nquads_content_type(headers: &HeaderMap) -> Result<(), PaperRegistrationError> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/n-quads"))
    {
        return Err(PaperRegistrationError::InvalidInput);
    }
    Ok(())
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, PaperRegistrationError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(PaperRegistrationError::InvalidSession)?
        .to_str()
        .map_err(|_| PaperRegistrationError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if token.trim().is_empty() {
                return Err(PaperRegistrationError::InvalidSession);
            }
            return Ok(token.to_string());
        }
    }

    Err(PaperRegistrationError::InvalidSession)
}

fn paper_associated_reference(doi: Option<&str>, title: Option<&str>) -> Option<String> {
    if let Some(doi) = doi.map(str::trim).filter(|value| !value.is_empty()) {
        let bare_doi = doi
            .strip_prefix("https://doi.org/")
            .or_else(|| doi.strip_prefix("http://doi.org/"))
            .or_else(|| doi.strip_prefix("doi:"))
            .unwrap_or(doi)
            .trim();
        if !bare_doi.is_empty() {
            return Some(format!("https://doi.org/{bare_doi}"));
        }
    }

    title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn add_paper_provenance(
    frontend_nquads: &[u8],
    paper_id: Uuid,
    associated_reference: &str,
) -> Result<Vec<u8>, PaperRegistrationError> {
    let mut quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(frontend_nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| PaperRegistrationError::InvalidRdf)?;

    let first = quads.first().ok_or(PaperRegistrationError::InvalidRdf)?;
    let subject = first.subject.clone();

    // The normal registration UI uses one temporary blank-node subject. Keep
    // provenance server-managed and reject attempts to submit those predicates.
    if !subject.is_blank_node()
        || quads.iter().any(|quad| quad.subject != subject)
        || quads.iter().any(|quad| {
            matches!(
                quad.predicate.as_str(),
                ASSOCIATED_REFERENCES_PREDICATE_URI | SOURCE_PAPER_PREDICATE_URI
            )
        })
    {
        return Err(PaperRegistrationError::InvalidRdf);
    }

    let graph = NamedNode::new(OCCURRENCE_GRAPH_URI)
        .map_err(|_| PaperRegistrationError::Internal)?;
    let associated_references = NamedNode::new(ASSOCIATED_REFERENCES_PREDICATE_URI)
        .map_err(|_| PaperRegistrationError::Internal)?;
    let source_paper = NamedNode::new(SOURCE_PAPER_PREDICATE_URI)
        .map_err(|_| PaperRegistrationError::Internal)?;
    let paper_uri = NamedNode::new(format!("{PAPER_URI_BASE}{paper_id}"))
        .map_err(|_| PaperRegistrationError::Internal)?;

    quads.push(Quad::new(
        subject.clone(),
        associated_references,
        Literal::new_simple_literal(associated_reference),
        GraphName::NamedNode(graph.clone()),
    ));
    quads.push(Quad::new(
        subject,
        source_paper,
        paper_uri,
        GraphName::NamedNode(graph),
    ));

    let mut serializer = RdfSerializer::from_format(RdfFormat::NQuads).for_writer(Vec::new());
    for quad in &quads {
        serializer
            .serialize_quad(quad)
            .map_err(|_| PaperRegistrationError::Internal)?;
    }
    serializer.finish().map_err(|_| PaperRegistrationError::Internal)
}

fn map_occurrence_error(error: OccurrenceServiceError) -> PaperRegistrationError {
    match error {
        OccurrenceServiceError::StoreFailed => PaperRegistrationError::StoreFailed,
        OccurrenceServiceError::RdfParseFailed
        | OccurrenceServiceError::FrontendManagedPredicateProvided
        | OccurrenceServiceError::ForbiddenRdfGraph
        | OccurrenceServiceError::EmptyRdf
        | OccurrenceServiceError::InvalidAccessRights
        | OccurrenceServiceError::InvalidLicense
        | OccurrenceServiceError::InvalidBlankNodeSubject
        | OccurrenceServiceError::InvalidObjectBlankNode => PaperRegistrationError::InvalidRdf,
        _ => PaperRegistrationError::Internal,
    }
}
