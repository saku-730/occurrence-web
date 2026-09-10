# associatedOrganisms

## 基本情報

IRI
`http://rs.tdwg.org/dwc/terms/associatedOrganisms`

ラベル
`Associated Organisms`

種別
`Property`

目的語の形式: リテラルを主に使用

## 定義

- 原文 -

A list (concatenated and separated) of identifiers of other dwc:Organism records and their associations to this dwc:Organism.

- 日本語訳 -

この生物個体に関連する他の `dwc:Organism` レコードの識別子と、その関係を区切って連結した一覧です。

## 説明

宿主、共生者、寄生者、同一個体の別記録など、現在の生物個体と関係する別の生物個体を示すための用語です。関係の種類と対象の識別子を対応付けて記録します。

## Bio-Databaseでの使い方

データ登録・編集画面で、別の生物個体との関係を文字列として補足する必要がある場合に使用します。複数の関係を厳密にモデル化する必要がない通常のオカレンス登録では、基本的に使用しません。

## 関連用語

- `dwc:organismID`：関連先生物個体の識別子
- `dwc:associatedOccurrences`：関連するオカレンス
- `dwc:organismInteractionType`：生物個体間の相互作用の種別

## 別名（日本語）

- 関連生物個体
- 関連個体

## リンク

[公式の用語一覧](https://dwc.tdwg.org/list/#dwc_associatedOrganisms)
