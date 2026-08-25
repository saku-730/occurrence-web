# associatedOccurrences

## 基本情報

IRI  
`http://rs.tdwg.org/dwc/terms/associatedOccurrences`

ラベル
`Associated Occurrences`

種別
`Property`

目的語の形式: リテラル（関連するオカレンスの識別子と関係の一覧）

## 定義

- 原文 -

A list (concatenated and separated) of identifiers of other dwc:Occurrence records and their associations to this dwc:Occurrence.

- 日本語訳 -

この`dwc:Occurrence`に関連する、他の`dwc:Occurrence`レコードの識別子と、その関係を区切って連結した一覧。

## 説明

あるオカレンスと関係する別のオカレンスを記録するための用語です。

関連するオカレンスの識別子と、両者がどのような関係にあるかを併せて記録します。

例えば、寄生生物のオカレンスについて、その寄生生物が採集された宿主のオカレンスを示す場合や、同じ個体について過去に記録された別のオカレンスを示す場合に使用できます。

複数の関係を詳しく管理する場合は、`dwc:ResourceRelationship`を使用する方法もあります。

## Bio-Databaseでの使い方

現在のオカレンスに関連する別のオカレンスがある場合に使用します。

例えば、寄生生物と宿主のオカレンスを関連付ける場合や、同じ個体について以前に登録されたオカレンスを示す場合に使用できます。

通常のオカレンスデータに関連する別のオカレンスがない場合は使用しません。

例 寄生：6fd1eb3a-13f0-4776-87b2-0fc9732f7f7a(OCCURRENCE ID)

## 関連用語

- `dwc:occurrenceID`：関連先オカレンスの識別子
- `dwc:associatedOrganisms`：関連する生物個体
- `dwc:relatedResourceID`：関連リソースの識別子

## 別名（日本語）

- 関連オカレンス

## リンク

[公式の用語一覧](https://dwc.tdwg.org/list/#dwc_associatedOccurrences)
