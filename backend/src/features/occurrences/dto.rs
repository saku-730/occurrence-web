use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// フロントがRDF編集前に使うID/URI/N-Quadsの下書き。保存はまだ行わない。
#[derive(Debug, Serialize, ToSchema)]
pub struct PrepareOccurrenceResponse {
    pub occurrence_id: String,
    pub occurrence_uri: String,
    pub nquads: String,
}

// 登録完了後は、保存されたoccurrenceを参照するためのIDとURIだけを返す。
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateOccurrenceResponse {
    pub occurrence_id: String,
    pub occurrence_uri: String,
}

// 物理削除の成否をHTTP bodyでも明示するための最小レスポンス。
#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteOccurrenceResponse {
    pub deleted: bool,
}

// 一覧表示用DTO。RDF全文ではなく、画面で頻繁に使う代表項目だけを返す。
#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct SearchOccurrenceItem {
    pub occurrence_id: String,
    pub occurrence_uri: String,
    // 権限判定用にstoreから受け取る内部値。APIレスポンスには出さない。
    #[serde(skip_serializing)]
    pub creator_user_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    pub basis_of_record: Option<String>,
    pub recorded_by: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub access_rights: Option<String>,
}

// cursor paginationの状態。offsetではなくopaque cursorを返し、store実装の詳細を隠す。
#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct SearchOccurrencesPage {
    #[schema(default = 50, example = 50)]
    pub limit: u32,
    pub next_cursor: Option<String>,
    pub has_next: bool,
}

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct SearchOccurrencesResponse {
    pub items: Vec<SearchOccurrenceItem>,
    pub page: SearchOccurrencesPage,
}

// MVPではfiltersとpageを明示的に受け取り、将来の検索条件追加に備える。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchOccurrencesRequest {
    pub filters: Vec<SearchOccurrenceFilter>,
    pub page: SearchOccurrencesRequestPage,
}

// predicate/value/value_type/matchで任意述語検索を表現する。現在matchはexactのみ許可する。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchOccurrenceFilter {
    pub predicate: String,
    pub value: String,
    pub value_type: String,
    pub r#match: String,
}

// limit未指定時はservice側で50に補完する。cursorは前ページのnext_cursorをそのまま渡す。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchOccurrencesRequestPage {
    #[schema(default = 50, example = 50)]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
