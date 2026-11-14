# C ABI（rustorb-dds-ffi）

rustorb/rustorb#111 の D4（#43）の 1 番目、#44 の設計をまとめる。C++ と Java のバインディングは、この C ABI の上に作る。rustorb の `rustorb-ffi` と同じく、Rust のコアは `extern "C"` の関数だけを公開し、言語ごとの API はその上に書く。Python バインディング（`rustorb-dds-python`）は PyO3 で直接 Rust の API を呼ぶので、この ABI は使わない。

| 場所 | 内容 |
| --- | --- |
| `crates/rustorb-dds-ffi` | C ABI（`librustorb_dds_ffi.a` / `.so` / `.dylib` / `.dll`）。依存は rustorb-dds、rustorb-xtypes、rustorb-cdr だけ |
| `crates/rustorb-dds-ffi/include/rustorb/dds.h` | ヘッダ。関数と構造体はすべて `rdds_` で始まる。呼び出し側の決まりはヘッダの先頭に書いてある |
| `crates/rustorb-dds-ffi/tests/abi.rs` | Rust から C と同じように呼ぶ試験 |
| `crates/rustorb-dds-ffi/tests/c` | C（C11）と C++（C++17）でヘッダを読み、送受信する試験（`run.sh`、CI の `c-abi`） |

## 使い方

```sh
cargo build --release -p rustorb-dds-ffi
cc -std=c11 -Icrates/rustorb-dds-ffi/include main.c \
   target/release/librustorb_dds_ffi.a -lpthread -ldl -lm
```

例は `tests/c/smoke.c`。

## 設計

### ハンドルとエラー

- 参加者、トピック、writer、reader、型、条件などはすべて不透明なハンドルで、作る関数が出力引数に入れる。`rdds_*_release` で一度だけ解放する。NULL の解放は何もしない。writer や reader を解放するとその entity は削除される
- 失敗しうる関数は DDS の `ReturnCode_t`（DDS 1.4, 2.2.1.1）をそのまま返す。C++ の PSM の例外（`dds::core::TimeoutError` など）や Java の例外にそのまま対応づけられる。理由の文は `rdds_last_error_message()` でスレッドごとに取る
- Rust の panic は境界で捕まえて `RETCODE_ERROR` にする

### 型

型は DDS-XTypes 1.3 の `DynamicTypeBuilder`（7.5.2.2）と同じく、部品から組み立てる。基本型は TypeKind（`RDDS_TK_INT32` など）、文字列・列・配列・写像・typedef はそれぞれの関数、enum・bitmask・struct・union はビルダーにリテラルやメンバーを一つずつ足す。メンバーの ID、`@key`・`@optional`・`@external`・`@must_understand`・`@try_construct`・`@hashid`、拡張性、`@nested`、`@autoid(HASH)` を渡せる。

生成コード（#45、#46）はこの関数で自分の型を組み立てる。組み立てた型の TypeIdentifier は、同じ IDL を rustorb-idl の Rust 出力が記述したものと一致する（`tests/abi.rs`）。`rdds_type_hashes` で確かめられる。

### サンプル：値のストリーム

サンプルは、型の順に値を書く出力ストリーム（`rdds_out`）と、同じ順に読む入力ストリーム（`rdds_in`）でやりとりする。

- struct はメンバーを順に（基底のメンバーが先）。`@optional` のメンバーは、まず有無（`rdds_out_present`）、あればその値
- union は判別子（`rdds_out_discriminator`）と、それが選ぶメンバーの値
- sequence と map は長さ（`rdds_out_length`）と要素。map は key、value の順
- 配列は全要素を行の順に
- 要素が基本型（`float128` 以外）の sequence と配列は、要素をまとめて一度に渡す（`rdds_out_int32_array` など）。sequence では長さの後
- enum はリテラルの値、bitmask はビット
- typedef と `@external` は透過

**この形にした理由**：符号化（XCDR1 と XCDR2、DHEADER、EMHEADER、mutable の各メンバーのヘッダー、キーハッシュ）を Rust 側に一か所だけ置くため。生成コードは値を順に書くだけで、符号化のことを知らない。C++ と Java の生成器は簡単になり、XCDR の正しさは Rust 側の試験（D3 で生成コードとバイト単位で一致を確かめた DynamicData の直列化）にまかせられる。rustorb の `rorb_out` / `rorb_in`（GIOP の CDR）と同じ使い勝手だが、こちらはストリームが型を知らず、writer が書くときに型と照らし合わせる。型に合わない値は `RETCODE_BAD_PARAMETER` で、どこが合わないかを文で返す。

**今の実装**：ストリームはいったん値の列にため、writer が型をたどって `DynamicData` を組み立ててから直列化する。読むときは逆に `DynamicData` から値の列を作る。ABI はこの内部に依存しないので、後で型をたどりながら直接 XCDR に書く形に変えても呼び出し側は変わらない。

### QoS、状態、listener

- QoS は値の構造体（`rdds_writer_qos`、`rdds_reader_qos`、`rdds_group_qos`、`rdds_participant_qos`）で渡す。`rdds_*_qos_default` で既定値を取り、変えたい所だけ書き換える。長さは DDS と同じく `RDDS_LENGTH_UNLIMITED`（-1）、期間は `Duration_t`（無限は `0x7fffffff` 秒・`0x7fffffff` ナノ秒）
- `rdds_participant_qos` の `property` と `property_len` は PROPERTY（DDS Security 1.2、7.2.5）の `rdds_property`（`name`、`value`）の配列。`dds.sec.*` の 6 つで DDS Security を有効にする（`docs/security.md`）。ライブラリは既定で feature `security` 付き
- 状態は構造体で返す。取得すると変化の数は 0 に戻る（DDS 1.4 どおり）。不整合 QoS の方針の一覧は固定長（32）の配列で、所有の受け渡しを避けた
- listener は関数表と `self`。どの関数も NULL でよい。ライブラリのスレッドから呼ばれる。ただし同じ参加者の writer が書いたときは、その `rdds_writer_*` の呼び出しの中で、呼び出し側のスレッドから呼ばれる。ライブラリのロックは持たないので、listener からどの関数を呼んでもよい。`release` は listener が置き換えられたとき、または entity が削除されたときに一度だけ呼ばれる。C++ と Java はここで自分のオブジェクトの参照を手放す
- 参加者・publisher・subscriber・topic の listener は一つの関数表 `rdds_listener`（`rdds_*_set_listener`）。writer と reader の状態では `rdds_endpoint_info`（GUID、トピック名、型名）を渡す。entity の listener が受けない状態は publisher・subscriber、次に参加者の listener へ（DDS 1.4 2.2.4.3）。`on_data_on_readers` に渡す subscriber は呼び出しの間だけ有効（残すなら `rdds_subscriber_clone`、比べるのは `rdds_subscriber_same`）。`rdds_subscriber_notify_datareaders` で reader の listener を呼ぶ
- 読み取りは `rdds_reader_select`（状態のマスク、インスタンス指定、次のインスタンス、`max_samples`）と `rdds_reader_select_w_condition`。結果の `rdds_samples` からサンプル情報と、サンプルごとの入力ストリームを取る

### Content filtered topic

`rdds_participant_create_contentfilteredtopic` は `rdds_topic` からフィルタ式（DDS 1.4 付録 B）とその引数の文字列で `rdds_cftopic` を作る。その reader は `rdds_subscriber_create_filtered_reader` で作り、ほかの reader と同じに読む。引数は `rdds_cftopic_set_expression_parameters` で変えられる。式が型に合わなければ `RDDS_RETCODE_BAD_PARAMETER`。

### 要求と応答（DDS-RPC）

`rdds_requester_new` と `rdds_replier_new` は要求と応答の 2 つの `rdds_topic` から作る（QoS は NULL で RELIABLE、KEEP_ALL）。要求は `rdds_requester_send_request` が返す `rdds_sample_identity`（書き手の GUID と sequence number）で識別し、`rdds_requester_receive_reply` でその応答を待つ。サービス側は `rdds_replier_receive_request` で要求を 1 件ずつ受け、受け取った identity を `rdds_replier_send_reply` に渡す。待ち時間が尽きれば `RDDS_RETCODE_TIMEOUT`。標本の related sample identity は `rdds_samples_related_sample_identity` で読める。詳しくは [rpc.md](rpc.md)。

### WaitSet と条件

guard・status・read の各条件は一つのハンドル型 `rdds_condition`。`rdds_waitset_wait` は真になった条件を、付けた順の番号で返す。言語側は番号から自分の条件オブジェクトを引く。

## まだないもの

- 型の中身を調べる関数（受け取った `DynamicType` を言語側から見る）。C++/Java で DynamicData を扱うときに足す
- `discover_topic_type`（TypeLookup で相手の型を取る）。同上
