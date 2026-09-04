// 外部システムとの接続実装。occurrence RDF storeとしてFusekiを使う。
// Digital Agency ABR PostgreSQLを行政区分マスターとして直接参照する。
pub mod abr;
// Darwin Core候補、GBIF分類階層検索、GBIF学名解決を適用する。
#[path = "fuseki_searchable.rs"]
pub mod fuseki;
// Garage/S3互換object storageとの通信実装と実接続テストを置く。
pub mod garage;
// NominatimへのGeocoding requestを直列化し、同一queryをprocess内cacheする。
pub mod nominatim;
