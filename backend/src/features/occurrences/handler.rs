use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{
        HeaderMap, StatusCode, header,
        header::{CONTENT_TYPE, COOKIE},
    },
    response::{IntoResponse, Response},
};
use oxrdf::Term;
use oxrdfio::{RdfFormat, RdfParser};
use uuid::Uuid;

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

use super::{
    dto::{CreateOccurrenceResponse, SearchOccurrencesRequest, SearchOccurrencesResponse},
    service::{
        CreateOccurrenceInput, GetOccurrenceInput, OccurrenceService, OccurrenceServiceError,
        SearchOccurrencesInput,
    },
};

#[derive(Debug)]
pub enum OccurrenceHandlerError {
    InvalidSession,        //セッションを持っていないなど
    Database(sqlx::Error), //posgre側のエラー トークン認証など
    NotImplemented,        //
    UnsupportedMediaType,  //httpリクエストのbodyがtext/turtle以外など
    EmptyBody,             //httpリクエストのbodyが空
    InternalServerError,   //サーバー側の処理エラーなど
    InvalidRdf,            //フロントから送信されたN-Quadsが壊れている
    RdfStoreError,         //
    ForbiddenRdfPredicate, //禁止されている述語を含むRDFを拒否
    ForbiddenRdfGraph,     //グラフ名が間違っている場合拒否
    EmptyRdf,              //空のデータを拒否
    InvalidAccessRights,    //accessRightsが仕様外
    InvalidLicense,         //licenseが仕様外
    InvalidBlankNodeSubject, //blank node subjectが仕様外
    InvalidObjectBlankNode,  //object blank nodeは拒否
    NotFound,              //
}

impl From<AuthServiceError> for OccurrenceHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<OccurrenceServiceError> for OccurrenceHandlerError {
    fn from(error: OccurrenceServiceError) -> Self {
        match error {
            OccurrenceServiceError::NotImplemented => Self::NotImplemented,
            OccurrenceServiceError::InvalidOccurrenceUri => Self::InternalServerError,
            OccurrenceServiceError::InvalidPredicateUri => Self::InternalServerError,
            OccurrenceServiceError::InvalidUserUri => Self::InternalServerError,
            OccurrenceServiceError::InvalidGraphUri => Self::InternalServerError,
            OccurrenceServiceError::RdfSerializationFailed => Self::InternalServerError,
            OccurrenceServiceError::RdfParseFailed => Self::InvalidRdf,
            OccurrenceServiceError::StoreFailed => Self::RdfStoreError,
            OccurrenceServiceError::FrontendManagedPredicateProvided => Self::ForbiddenRdfPredicate,
            OccurrenceServiceError::ForbiddenRdfGraph => Self::ForbiddenRdfGraph,
            OccurrenceServiceError::EmptyRdf => Self::EmptyRdf,
            OccurrenceServiceError::InvalidAccessRights => Self::InvalidAccessRights,
            OccurrenceServiceError::InvalidLicense => Self::InvalidLicense,
            OccurrenceServiceError::InvalidBlankNodeSubject => Self::InvalidBlankNodeSubject,
            OccurrenceServiceError::InvalidObjectBlankNode => Self::InvalidObjectBlankNode,
        }
    }
}

impl IntoResponse for OccurrenceHandlerError {
    fn into_response(self) -> Response {
        match self {
            OccurrenceHandlerError::InvalidSession => {
                let body = ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                };

                (StatusCode::UNAUTHORIZED, Json(body)).into_response()
            }

            OccurrenceHandlerError::Database(_) => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
            OccurrenceHandlerError::NotImplemented => {
                let body = ErrorResponse {
                    error: "not_implemented".to_string(),
                    message: "Occurrence creation is not implemented yet".to_string(),
                };

                (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response()
            }
            OccurrenceHandlerError::UnsupportedMediaType => {
                let body = ErrorResponse {
                    error: "unsupported_media_type".to_string(),
                    message: "Unsupported media type".to_string(),
                };

                (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(body)).into_response()
            }
            OccurrenceHandlerError::EmptyBody => {
                let body = ErrorResponse {
                    error: "empty_body".to_string(),
                    message: "Request body must not be empty".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::InternalServerError => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
            OccurrenceHandlerError::RdfStoreError => {
                let body = ErrorResponse {
                    error: "rdf_store_error".to_string(),
                    message: "Failed to save occurrence RDF".to_string(),
                };

                (StatusCode::BAD_GATEWAY, Json(body)).into_response()
            }
            OccurrenceHandlerError::ForbiddenRdfPredicate => {
                let body = ErrorResponse {
                    error: "forbidden_rdf_predicate".to_string(),
                    message: "Frontend RDF must not contain backend-managed predicates".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::ForbiddenRdfGraph => {
                let body = ErrorResponse {
                    error: "forbidden_rdf_graph".to_string(),
                    message: "Occurrence RDF must use the occurrence graph".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            Self::InvalidRdf => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_rdf".to_string(),
                    message: "Invalid RDF body".to_string(),
                }),
            )
                .into_response(),
            OccurrenceHandlerError::EmptyRdf => {
                let body = ErrorResponse {
                    error: "empty_rdf".to_string(),
                    message: "Occurrence RDF must contain at least one quad".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::InvalidAccessRights => {
                let body = ErrorResponse {
                    error: "invalid_access_rights".to_string(),
                    message: "Invalid access rights".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::InvalidLicense => {
                let body = ErrorResponse {
                    error: "invalid_license".to_string(),
                    message: "Invalid license".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::InvalidBlankNodeSubject => {
                let body = ErrorResponse {
                    error: "invalid_blank_node_subject".to_string(),
                    message: "Invalid blank node subject".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::InvalidObjectBlankNode => {
                let body = ErrorResponse {
                    error: "invalid_object_blank_node".to_string(),
                    message: "Invalid object blank node".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            OccurrenceHandlerError::NotFound => {
                let body = ErrorResponse {
                    error: "occurrence_not_found".to_string(),
                    message: "Occurrence not found".to_string(),
                };

                (StatusCode::NOT_FOUND, Json(body)).into_response()
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/occurrences",
    request_body(
        content = String,
        content_type = "application/n-quads",
        description = "Occurrence RDF in N-Quads format. All quads must use <https://bio-database.net/graphs/occurrences> as the graph name. Backend-managed predicates such as dcterms:creator must not be included."
    ),
    responses(
        (
            status = 201,
            description = "Occurrence created",
            body = CreateOccurrenceResponse
        ),
        (
            status = 400,
            description = "Invalid occurrence RDF body",
            body = ErrorResponse
        ),
        (
            status = 401,
            description = "Authentication required",
            body = ErrorResponse
        ),
        (
            status = 415,
            description = "Content-Type must be application/n-quads",
            body = ErrorResponse
        ),
        (
            status = 502,
            description = "Failed to save occurrence RDF to RDF store",
            body = ErrorResponse
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse
        )
    ),
    tag = "occurrences"
)]

pub async fn create_occurrence(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<CreateOccurrenceResponse>), OccurrenceHandlerError> {
    let session_token = extract_session_token(&headers)?;

    let current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let content_type = ensure_supported_rdf_content_type(&headers)?;
    ensure_non_empty_body(&body)?;

    let input = CreateOccurrenceInput {
        create_user_id: current_user.user_id,
        content_type,
        rdf_body: body.to_vec(),
    };

    let output =
        OccurrenceService::create_occurrence(input, state.occurrence_rdf_store.as_ref()).await?;

    let response = CreateOccurrenceResponse {
        occurrence_id: output.occurrence_id.to_string(),
        occurrence_uri: output.occurrence_uri,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn search_occurrences(
    State(state): State<AppState>,
    Json(request): Json<SearchOccurrencesRequest>,
) -> Result<Json<SearchOccurrencesResponse>, OccurrenceHandlerError> {
    let input = SearchOccurrencesInput {
        limit: request.page.limit,
        cursor: request.page.cursor,
    };

    let output =
        OccurrenceService::search_occurrences(input, state.occurrence_rdf_store.as_ref()).await?;

    Ok(Json(output))
}

#[utoipa::path(
    get,
    path = "/occurrences/{occurrence_id}",
    params(
        (
            "occurrence_id" = Uuid,
            Path,
            description = "Occurrence UUID"
        )
    ),
    responses(
        (
            status = 200,
            description = "Occurrence RDF in N-Quads format",
            body = String,
            content_type = "application/n-quads"
        ),
        (
            status = 400,
            description = "Invalid occurrence UUID",
            body = ErrorResponse
        ),
        (
            status = 404,
            description = "Occurrence not found or private occurrence is hidden",
            body = ErrorResponse
        ),
        (
            status = 502,
            description = "Failed to read occurrence RDF from RDF store",
            body = ErrorResponse
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse
        )
    ),
    tag = "occurrences"
)]
pub async fn get_occurrence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(occurrence_id): Path<Uuid>,
) -> Result<Response, OccurrenceHandlerError> {
    let input = GetOccurrenceInput { occurrence_id };

    let output =
        OccurrenceService::get_occurrence(input, state.occurrence_rdf_store.as_ref()).await?;

    let Some(output) = output else {
        return Err(OccurrenceHandlerError::NotFound);
    };

    if nquads_contains_private_access_rights(&output.nquads)? {
        let Some(session_token) = optional_session_token(&headers) else {
            // private occurrenceは存在自体を隠す仕様なので、非ログインには404を返す。
            return Err(OccurrenceHandlerError::NotFound);
        };

        let current_user = match AuthService::current_user(&state.posgre, session_token).await {
            Ok(current_user) => current_user,
            Err(AuthServiceError::InvalidSession) => return Err(OccurrenceHandlerError::NotFound),
            Err(error) => return Err(error.into()),
        };

        let Some(creator_user_id) = nquads_creator_user_id(&output.nquads)? else {
            return Err(OccurrenceHandlerError::NotFound);
        };

        if current_user.role != "admin" && current_user.user_id != creator_user_id {
            return Err(OccurrenceHandlerError::NotFound);
        }
    }

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/n-quads")],
        output.nquads,
    )
        .into_response())
}

const ACCESS_RIGHTS_PREDICATE_URI: &str = "http://purl.org/dc/terms/accessRights";
const CREATOR_PREDICATE_URI: &str = "http://purl.org/dc/terms/creator";
const PRIVATE_ACCESS_RIGHTS_URI: &str = "https://bio-database.net/terms/access-rights/private";
const USER_URI_BASE: &str = "https://bio-database.net/users/";

fn nquads_contains_private_access_rights(nquads: &[u8]) -> Result<bool, OccurrenceHandlerError> {
    let quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OccurrenceHandlerError::InternalServerError)?;

    Ok(quads.iter().any(|quad| {
        quad.predicate.as_str() == ACCESS_RIGHTS_PREDICATE_URI
            && matches!(
                &quad.object,
                Term::NamedNode(access_rights)
                    if access_rights.as_str() == PRIVATE_ACCESS_RIGHTS_URI
            )
    }))
}

fn nquads_creator_user_id(nquads: &[u8]) -> Result<Option<Uuid>, OccurrenceHandlerError> { //nquadsからuseridだけ取り出し
    let quads = RdfParser::from_format(RdfFormat::NQuads)
        .for_slice(nquads)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OccurrenceHandlerError::InternalServerError)?;

    let creator_user_id = quads.iter().find_map(|quad| {
        if quad.predicate.as_str() != CREATOR_PREDICATE_URI {
            return None;
        }

        let Term::NamedNode(creator) = &quad.object else {
            return None;
        };

        creator //取り出した、uriのuser_idを整形
            .as_str()
            .strip_prefix(USER_URI_BASE)
            .and_then(|user_id| Uuid::parse_str(user_id).ok())
    });

    Ok(creator_user_id)
}

fn optional_session_token(headers: &HeaderMap) -> Option<String> {
    extract_session_token(headers).ok()
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, OccurrenceHandlerError> {
    //トークン取り出し ヘルパー
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(OccurrenceHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| OccurrenceHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        //session=asdfasdf; user=asdfasdf;...って感じのヘッダー
        let cookie = cookie.trim(); //cookie整形

        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(OccurrenceHandlerError::InvalidSession);
            }

            return Ok(session_token.to_string());
        }
    }

    Err(OccurrenceHandlerError::InvalidSession)
}

fn ensure_supported_rdf_content_type(
    //content-typeを確認 text/turtle以外はエラー
    headers: &HeaderMap,
) -> Result<String, OccurrenceHandlerError> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .ok_or(OccurrenceHandlerError::UnsupportedMediaType)?
        .to_str()
        .map_err(|_| OccurrenceHandlerError::UnsupportedMediaType)?;

    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match media_type.as_str() {
        "application/n-quads" => Ok(media_type),
        _ => Err(OccurrenceHandlerError::UnsupportedMediaType),
    }
}

fn ensure_non_empty_body(body: &Bytes) -> Result<(), OccurrenceHandlerError> {
    // HTTP body が完全に空でないことだけを確認する。
    // 空白だけ・コメントだけなど、RDF quad が 0 件になるケースは service 側で EmptyRdf として扱う。
    if body.is_empty() {
        return Err(OccurrenceHandlerError::EmptyBody);
    }

    Ok(())
}
