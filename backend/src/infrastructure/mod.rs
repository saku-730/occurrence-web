// 外部システムとの接続実装。今はoccurrence RDF storeとしてFusekiだけを持つ。
// Darwin Core候補取得はBio-Database固有のoccurrence-profile graphを適用する。
#[path = "fuseki_profiled.rs"]
pub mod fuseki;
// Garage/S3互換object storageとの通信実装と実接続テストを置く。
pub mod garage;
