// 外部システムとの接続実装。occurrence RDF storeとしてFusekiを使う。
// Darwin Core候補取得はBio-Database固有のoccurrence-profile graphを適用する。
#[path = "fuseki_profiled.rs"]
pub mod fuseki;
// Garage/S3互換object storageとの通信実装と実接続テストを置く。
pub mod garage;
// NominatimへのGeocoding requestを直列化し、同一queryをprocess内cacheする。
pub mod nominatim;
