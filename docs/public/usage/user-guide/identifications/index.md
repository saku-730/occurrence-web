# Identifications<no value>

## Identificationとは何か

Identificationは、オカレンスに対する生物学的な同定情報を表すまとまりです。オカレンス本体とは分離して、学名、同定者、同定日、確度、注記などを管理します。

## 現在の同定

現在採用している学名や分類群を、オカレンスのIdentificationとして記録します。オカレンス本体の記録情報と分離することで、採集・観察の事実と同定結果を区別できます。

## 複数同定の扱い

現行のMVPでは、各オカレンスに1件のIdentificationを登録します。データモデルでは複数のIdentificationを扱える識別子の構造を採用しており、将来は同定履歴を複数件として記録する予定です。

## 同定者

`dwc:identifiedBy` を使って、同定を行った人または組織を記録します。

## 同定日

`dwc:dateIdentified` を使って、同定が行われた日付を記録します。

## 同定確度・注記

`dwc:identificationQualifier` で同定の確度や限定条件を、`dwc:identificationRemarks` で補足情報を記録します。

## GBIF分類群との関連付け

分類をGBIF Backbone Taxonomyの候補から選ぶと、`dwciri:toTaxon` にGBIFの分類群URIを記録します。同時に学名を `dwc:scientificName` として保存し、URIと表示用の学名を関連付けます。
