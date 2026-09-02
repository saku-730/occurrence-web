// 機能単位の境界。authはユーザー/セッション、occurrencesはRDF occurrenceデータを担当する。
pub mod auth;
pub mod occurrences;
// occurrence_mapはOccurrence登録時の地名Geocodingと地図用GeoJSON APIを担当する。
pub mod occurrence_map;
// mediaは添付ファイルのmetadataとobject storage連携を担当する。
pub mod media;
// paper_importは論文PDFの受信から論文由来データのimport処理を担当する。
pub mod paper_import;
