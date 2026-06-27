// 外部システムとの接続実装。今はoccurrence RDF storeとしてFusekiだけを持つ。
pub mod fuseki;
// Garage/S3互換object storageとの通信実装と実接続テストを置く。
pub mod garage;
