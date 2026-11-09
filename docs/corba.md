# CORBA の型と値（rustorb-dds-corba）

rustorb/rustorb#111 の D4（#43）の 4 番目、#47 の設計をまとめる。rustorb（ORB）の TypeCode と any を、DDS-XTypes 1.3 の型（`DynamicType`、そこから TypeObject）と `DynamicData` に変換し、その逆も行う。同じ型を CORBA と DDS の両方で使うため（OpenRTM-aist 2.0、#48）。

ORB に依存するのはこのクレートだけで、`rustorb-core` 0.3（MIT OR Apache-2.0）を使う。DDS のほかのクレートは `rustorb-cdr` と `rustorb-typesupport` にしか依存しない。`rustorb-core` は tokio と rustls も引き込むので、依存のライセンスに ISC（ring、rustls-webpki、untrusted）、BSD-3-Clause（subtle）、Unicode-3.0（unicode-ident）が加わる。

## 使い方

```rust
use rustorb_dds_corba::{Options, any, dynamic_data, dynamic_type, type_code};
use rustorb_xtypes::types::{Extensibility, TypeObjects};

let options = Options::new()
    .default_extensibility(Extensibility::Final)
    .key("Keyed::Shape", "id");
let ty = dynamic_type(&tc, &options)?;           // TypeCode -> DynamicType
let objects = TypeObjects::new(ty.as_type())?;   // TypeObject、TypeIdentifier
let data = dynamic_data(&value, &ty)?;           // Any -> DynamicData
let value = any(&data, &tc)?;                    // DynamicData -> Any
let tc = type_code(&ty)?;                        // DynamicType -> TypeCode
```

TypeObject から TypeCode を得るには、`DynamicTypeBuilderFactory::create_type_w_type_object` で `DynamicType` にしてから `type_code` を呼ぶ。

## 型の対応

TypeCode には extensibility、`@key`、`@optional`、member ID がない。足りないものは次の規則で決める。

| TypeCode | DDS-XTypes |
| --- | --- |
| `boolean`、`octet`、`short`...`unsigned long long`、`float`、`double`、`long double`、`char`、`wchar` | 同じ名前のプリミティブ型（`octet` は `TK_BYTE`、`long double` は `TK_FLOAT128`、`char` は `TK_CHAR8`、`wchar` は `TK_CHAR16`） |
| `string<N>`、`wstring<N>` | 同じ上限の文字列 |
| `sequence<T, N>` | 同じ上限の列 |
| 配列の配列（`long m[2][3]` の TypeCode は `array(array(long, 3), 2)`） | 多次元の配列ひとつ（次元 `[2, 3]`）。IDL を読んだときと同じ |
| `alias`、`enum`、`struct`、`union` | 同じ名前の型 |

- 名前：リポジトリ ID から修飾名を作る。`IDL:A/B/C:1.0` は `A::B::C`。先頭の `.` を含む部分（`#pragma prefix`）は除く。リポジトリ ID が `IDL:` でないか、型の名前で終わらないときは TypeCode の名前を使う。逆は `A::B::C` から `IDL:A/B/C:1.0` を作る（prefix は戻らない）
- extensibility：struct と union は `Options` のもの。既定は IDL 4 の既定と同じ `@appendable`。`Options::default_extensibility` で全体を、`Options::extensibility` で型ごとに変える。enum は rustorb-idl と同じく `@final`
- key：`Options::key(型, member)` で指定した member を `@key` にする。ない member を指定すると `Error::Mismatch`
- member ID：宣言の順に 0 から（`@autoid(SEQUENTIAL)`）。union の member も 0 から。rustorb-idl と Cyclone DDS の idlc と同じ
- enum の値：位置。union の enum のラベルも同じ（TypeCode のラベルは位置、DDS-XTypes のラベルは値なので、逆向きは値から位置を探す）
- union：TypeCode で続けて並ぶ同じ名前の member（ラベルが複数ある member）はひとつの member にまとめる。ラベルのない member（`default`）は `default`
- 同じリポジトリ ID の型は一度だけ作り、共有する

DDS-XTypes に型のないもの（`any`、`TypeCode`、オブジェクト参照、valuetype、`fixed`、例外、`Principal`...）と再帰する型は `Error::Unsupported`。逆向きでは `map`、`bitmask`、`int8`、`@optional` の member が `Error::Unsupported`。`uint8` は `octet` になる。struct の継承は基底の member を先に並べて平らにする。`@external`、`@try_construct`、`@bit_bound`、`@value`（enum の値）、member の注釈は TypeCode に残らない。

この規則と、IDL に同じ注釈（`@final`、`@key` など）を付けたときの rustorb-idl の型は一致し、TypeObject と TypeIdentifier も一致する。試験（`crates/rustorb-dds-corba/tests/convert.rs`）は、Cyclone DDS の fixture（`HelloWorld::Greeting`、`Keyed::Shape`、`Gen::Plain`、`Gen::Inner`、`Gen::U`）について、TypeCode から作った型が rustorb-idl の生成コード（`crates/rustorb-xtypes/tests/generated`、TypeObject が idlc と一致することは rustorb-xtypes の試験が確かめる）の型と等しく、TypeObject と TypeInformation も等しいことを確かめる。

## 値の対応

`dynamic_data(any, ty)` は any の値を `ty` の値にする。`ty` は `dynamic_type` が any の TypeCode から作った型（か、名前と形が同じ型）。`any(data, tc)` はその逆で、`tc` は `type_code` が作った TypeCode（か、名前と形が同じもの）。

- struct の member は名前で対応させる。TypeCode にない member は既定値のまま
- enum：any の値（位置）を enumerator の値にする。逆は値から位置
- union：any の値の member（TypeCode の member の番号）を名前で探し、判別子もそのまま移す。判別子が選ぶ member と any の member が違えば `Error::Mismatch`。逆は `TypeCode::union_member_for` で TypeCode の member の番号を選ぶ
- 列と配列：配列は平らにして入れ、戻すときに次元ごとに分ける。`octet` と `char` の列・配列は any では `Octets`、`Chars`（rustorb が CDR から読む形）にする
- `wchar` は UTF-16 の 1 単位。サロゲート（BMP の外）は `Error::Unsupported`
- `long double` は binary128 のビットをそのまま移す
- 文字列の上限は `DynamicData` が確かめる

試験は、any から作った `DynamicData` の XCDR1・XCDR2 のペイロードと key hash が、生成コードが同じ値を書いたものと一致すること、ワイド文字のない `@final` の型では XCDR1 のペイロードが rustorb の CDR（GIOP 1.2）と同じオクテットになること、any と `DynamicData` が往復で元に戻ることを確かめる。
