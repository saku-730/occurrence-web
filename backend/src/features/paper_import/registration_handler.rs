use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{
        HeaderMap, StatusCode,
        header::{CONTENT_TYPE, COOKIE},
    },
    response::{IntoResponse, Response},
};
use oxrdf::{GraphName, Literal, NamedNode, Quad, Term};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        media::repository::MediaRepository,
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
const SCIENTIFIC_NAME_PREDICATE_URI: &str = "http://rs.tdwg.org/dwc/terms/scientificName";
const SOURCE_PAPER_PREDICATE_URI: &str = "https://bio-database.net/terms/sourcePaper";
const PAPER_URI_BASE: &str = "https://bio-database.net/papers/";
const MAX_BATCH_OCCURRENCES: usize = 1000;

#[derive(Debug, Deserialize)]
pub struct RegisterPaperOccurrencesBatchRequest {
    pub occurrences: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterPaperOccurrencesBatchResponse {
    pub occurrences: Vec<CreateOccurrenceResponse>,
}

#[derive(Debug)]
pub enum PaperRegistrationError {
    InvalidSession,
    InvalidInput,
    NotFound,
    InvalidRdf,
    ForbiddenMedia,
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
            Self::ForbiddenMedia => (
                StatusCode::FORBIDDEN,
                "forbidden_media",
                "Occurrence media must be owned by the authenticated user",
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
    let associated_reference = load_associated_reference(&state, paper_id).await?;

    let response = register_one_occurrence(
        &state,
        paper_id,
        current_user.user_id,
        &associated_reference,
        &body,
    )
    .await?;

    if !PaperRepository::mark_registered(&state.posgre, paper_id).await? {
        return Err(PaperRegistrationError::NotFound);
    }

    Ok((StatusCode::CREATED, Json(response)))
}

/// Register every edited LLM candidate in one request. The paper status is
/// changed only after every requested occurrence has reached Fuseki.
pub async fn register_occurrences_batch(
    State(state): State<AppState>,
    Path(paper_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<RegisterPaperOccurrencesBatchRequest>,
) -> Result<(StatusCode, Json<RegisterPaperOccurrencesBatchResponse>), PaperRegistrationError> {
    if request.occurrences.is_empty() || request.occurrences.len() > MAX_BATCH_OCCURRENCES {
        return Err(PaperRegistrationError::InvalidInput);
    }

    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;
    let associated_reference = load_associated_reference(&state, paper_id).await?;
    let mut registered = Vec::with_capacity(request.occurrences.len());

    for nquads in request.occurrences {
        if nquads.trim().is_empty() {
            return Err(PaperRegistrationError::InvalidInput);
        }
        let response = register_one_occurrence(
            &state,
            paper_id,
            current_user.user_id,
            &associated_reference,
            nquads.as_bytes(),
        )
        .await?;
        registered.push(response);
    }

    if !PaperRepository::mark_registered(&state.posgre, paper_id).await? {
        return Err(PaperRegistrationError::NotFound);
    }

    Ok((
        StatusCode::CREATED,
        Json(RegisterPaperOccurrencesBatchResponse {
            occurrences: registered,
        }),
    ))
}

async fn load_associated_reference(
    state: &AppState,
    paper_id: Uuid,
) -> Result<String, PaperRegistrationError> {
    let paper = PaperRepository::find_by_id(&state.posgre, paper_id)
        .await?
        .ok_or(PaperRegistrationError::NotFound)?;

    paper_associated_reference(paper.doi.as_deref(), paper.title.as_deref())
        .ok_or(PaperRegistrationError::InvalidInput)
}

async fn register_one_occurrence(
    state: &AppState,
    paper_id: Uuid,
    user_id: Uuid,
    associated_reference: &str,
    body: &[u8],
) -> Result<CreateOccurrenceResponse, PaperRegistrationError> {
    ensure_referenced_media_owned_by_user(
        body,
        &state.config.app.app_base_url,
        user_id,
        &state.posgre,
    )
    .await?;

    // Taxonomy has already been resolved and shown to the user before this
    // point. Persist the reviewed RDF, normalizing scientificName to omit the
    // nomenclatural publication year, then add only paper provenance.
    let rdf_body = add_paper_provenance(body, paper_id, associated_reference)?;

    let output = OccurrenceService::create_occurrence(
        CreateOccurrenceInput {
            create_user_id: user_id,
            content_type: "application/n-quads".to_string(),
            rdf_body,
        },
        state.occurrence_rdf_store.as_ref(),
    )
    .await
    .map_err(map_occurrence_error)?;

    Ok(CreateOccurrenceResponse {
        occurrence_id: output.occurrence_id.to_string(),
        occurrence_uri: output.occurrence_uri,
    })
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

async fn ensure_referenced_media_owned_by_user(
    nquads: &[u8],
    app_base_url: &str,
    user_id: Uuid,
    db: &sqlx::PgPool,
) -> Result<(), PaperRegistrationError> {
    let quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| PaperRegistrationError::InvalidRdf)?;
    let media_uri_base = format!("{}/media/", app_base_url.trim().trim_end_matches('/'));
    let mut media_ids = std::collections::HashSet::new();

    for quad in quads {
        let Term::NamedNode(object) = quad.object else {
            continue;
        };
        let Some(media_id) = object.as_str().strip_prefix(&media_uri_base) else {
            continue;
        };
        let media_id =
            Uuid::parse_str(media_id).map_err(|_| PaperRegistrationError::ForbiddenMedia)?;
        media_ids.insert(media_id);
    }

    for media_id in media_ids {
        let metadata = MediaRepository::find_by_id(db, media_id).await?;
        if !metadata.is_some_and(|metadata| metadata.uploaded_by == user_id) {
            return Err(PaperRegistrationError::ForbiddenMedia);
        }
    }

    Ok(())
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

    if !subject.is_blank_node()
        || quads.iter().any(|quad| quad.subject != subject)
        || quads.iter().any(|quad| {
            let predicate = quad.predicate.as_str();
            predicate == ASSOCIATED_REFERENCES_PREDICATE_URI
                || predicate == SOURCE_PAPER_PREDICATE_URI
        })
    {
        return Err(PaperRegistrationError::InvalidRdf);
    }

    normalize_scientific_name_years(&mut quads);

    let graph =
        NamedNode::new(OCCURRENCE_GRAPH_URI).map_err(|_| PaperRegistrationError::Internal)?;
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

    serialize_nquads(&quads)
}

fn normalize_scientific_name_years(quads: &mut [Quad]) {
    for quad in quads {
        if quad.predicate.as_str() != SCIENTIFIC_NAME_PREDICATE_URI {
            continue;
        }
        let Term::Literal(literal) = &quad.object else {
            continue;
        };

        let normalized = scientific_name_without_year(literal.value());
        if normalized != literal.value() {
            quad.object = Literal::new_simple_literal(normalized).into();
        }
    }
}

fn scientific_name_without_year(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(without_closing_paren) = trimmed.strip_suffix(')') {
        if let Some(year_start) = terminal_year_start(without_closing_paren) {
            let author = without_closing_paren[..year_start]
                .trim_end_matches(|ch: char| ch == ',' || ch.is_whitespace());
            if author.ends_with('(') {
                return author.trim_end_matches('(').trim_end().to_string();
            }
            return format!("{author})");
        }
    }

    if let Some(year_start) = terminal_year_start(trimmed) {
        return trimmed[..year_start]
            .trim_end_matches(|ch: char| ch == ',' || ch.is_whitespace())
            .to_string();
    }

    trimmed.to_string()
}

fn terminal_year_start(value: &str) -> Option<usize> {
    let value = value.trim_end();
    let token_start = value
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    let token = &value[token_start..];
    let year = token.trim_end_matches(|ch: char| ch.is_ascii_alphabetic());

    if year.len() == 4 && year.chars().all(|ch| ch.is_ascii_digit()) {
        Some(token_start)
    } else {
        None
    }
}

fn serialize_nquads(quads: &[Quad]) -> Result<Vec<u8>, PaperRegistrationError> {
    let mut serializer = RdfSerializer::from_format(RdfFormat::NQuads).for_writer(Vec::new());
    for quad in quads {
        serializer
            .serialize_quad(quad)
            .map_err(|_| PaperRegistrationError::Internal)?;
    }
    serializer
        .finish()
        .map_err(|_| PaperRegistrationError::Internal)
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

#[cfg(test)]
mod tests {
    use super::scientific_name_without_year;

    #[test]
    fn removes_comma_and_terminal_year() {
        assert_eq!(scientific_name_without_year("Eisenia Malm, 1877"), "Eisenia Malm");
        assert_eq!(
            scientific_name_without_year("Pheretima acincta Goto & Hatai, 1899"),
            "Pheretima acincta Goto & Hatai"
        );
    }

    #[test]
    fn removes_year_inside_terminal_authorship_parentheses() {
        assert_eq!(
            scientific_name_without_year("Amynthas corticis (Kinberg, 1867)"),
            "Amynthas corticis (Kinberg)"
        );
    }

    #[test]
    fn leaves_names_without_year_unchanged() {
        assert_eq!(scientific_name_without_year("Eisenia Malm"), "Eisenia Malm");
    }
}
