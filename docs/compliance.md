# OMG 規格への対応状況

rustorb-dds が OMG の DDS 関連規格のどこまでを実装しているかを、規格ごとにまとめる。根拠はこのリポジトリのコード、`docs/` の各文書、[CHANGELOG.md](../CHANGELOG.md)、[interop/README.md](../interop/README.md) と、計画の issue（rustorb/rustorb#111、rustorb-dds#63）である。2026-11-21 時点、0.2.0 のあとの `main` について書く。

記号は次のとおり。

- ✅ 実装済み。試験がある。
- 🟡 部分的に実装。足りないところを書く。
- ❌ 未実装。
- — 対象外（rustorb-dds が実装するものではない）。

| 規格 | 版 | 状況 | 主なクレート・文書 |
|---|---|---|---|
| [DDS（DCPS）](#dds-14dcps) | 1.4 | 🟡 主要な部分 | `rustorb-dds` |
| [DDSI-RTPS](#ddsi-rtps-25) | 2.5 | ✅ UDPv4 の範囲で | `rustorb-rtps` |
| [DDS-XTypes](#dds-xtypes-13) | 1.3 | 🟡 主要な部分 | `rustorb-xtypes`、rustorb の `rustorb-cdr` |
| [DDS Security](#dds-security-12) | 1.2 | 🟡 組み込みプラグイン 3 つと Logging、Tagging | `rustorb-dds-security`、[security.md](security.md) |
| [DDS-RPC](#dds-rpc-10) | 1.0 | 🟡 Rust はほぼ全部、他言語は request/reply 形式 | `rustorb_dds::rpc`、[rpc.md](rpc.md) |
| [ISO/IEC C++ PSM for DDS](#isoiec-c-psm-for-dds) | 1.0 | 🟡 部分集合 | `cpp/`、[cpp.md](cpp.md) |
| [Java 5 PSM for DDS](#java-5-psm-for-dds) | 1.0（formal/2013-11-02） | 🟡 部分集合 | `java/`、[java.md](java.md) |
| [.NET と Ruby のバインディング](#net-と-ruby-のバインディング) | —（OMG の PSM はない） | 🟡 部分集合 | `dotnet/`、`ruby/`、[dotnet.md](dotnet.md)、[ruby.md](ruby.md) |
| [IDL4 と言語マッピング](#idl-4-と言語マッピング) | IDL 4.2 | 🟡 | rustorb-idl（別リポジトリ） |
| DDS-XML、DDS-JSON、DDS-WEB、DDS-XRCE、DDS-TSN など | — | ❌ | — |

相互接続の確認状況は[最後の節](#相互接続の確認状況)にまとめた。

## DDS 1.4（DCPS）

DCPS の API は Rust の `rustorb-dds` にあり、C ABI（[c-abi.md](c-abi.md)）を通して C++、Java、.NET、Ruby が、PyO3 を通して Python が使う。言語ごとの差は各言語の文書にある。

### エンティティと操作

| 項目 | 状況 | 備考 |
|---|---|---|
| DomainParticipant の作成、publisher・subscriber・topic・writer・reader の作成 | ✅ | 削除は Rust では drop、C++ は最後の参照、Java は `close()` |
| Topic、型の登録（`TypeSupport`） | ✅ | 型は rustorb の `rustorb-typesupport` のトレイトか DynamicType |
| ContentFilteredTopic（2.2.2.3.3、Annex B のフィルタ式） | ✅ | 全言語。SEDP で `PID_CONTENT_FILTER_PROPERTY` を告知。フィルタは reader 側で行う（#56） |
| MultiTopic | ❌ | |
| QueryCondition | ❌ | ReadCondition はある |
| `find_topic`、`lookup_topicdescription` | ❌ | Java の `lookupTopicDescription` は同じ参加者のトピックだけ |
| `ignore_participant`・`ignore_topic`・`ignore_publication`・`ignore_subscription` | ❌ | |
| 組み込みトピック（DCPSParticipant、DCPSTopic、DCPSPublication、DCPSSubscription）の reader、`get_builtin_subscriber` | ❌ | 代わりに `discovered_participants`、`matched_publications`、`matched_subscriptions` がある |
| `get_discovered_topics` | ❌ | |
| `get_current_time`、`assert_liveliness` | ✅ | |
| `set_qos`（作成後の QoS の変更） | ❌ | QoS は作成時に決める |
| `begin_coherent_changes`・`end_coherent_changes`、`begin_access`、`suspend_publications` | ❌ | PRESENTATION がないため |
| DataWriter: `write`、`write_w_timestamp`、`register_instance`、`unregister_instance`、`dispose`（`_w_timestamp` も）、`get_key_value`、`lookup_instance`、`wait_for_acknowledgments` | ✅ | |
| DataReader: `read`・`take`、`_instance`、`_next_instance`、`_w_condition`、`max_samples`、`get_key_value`、`lookup_instance` | ✅ | `max_samples` は take にも効く（#67） |
| `wait_for_historical_data` | 🟡 | Java はすぐ返す。TRANSIENT_LOCAL などの履歴は match 時に届く |
| SampleInfo（sample・view・instance state、各カウンタ、時刻、ハンドル） | ✅ | `related_sample_identity` も（DDS-RPC） |
| ゼロコピーの貸し出し（loan） | 🟡 | rustorb 独自の API（`loan_sample`、`take_loaned`）、Rust だけ、固定長の型だけ（[shm.md](shm.md)） |

### QoS ポリシー

| ポリシー | 状況 | 備考 |
|---|---|---|
| USER_DATA | ✅ | 参加者・writer・reader |
| TOPIC_DATA、GROUP_DATA | ❌ | PID の定数だけ |
| PARTITION | ✅ | publisher・subscriber |
| RELIABILITY | ✅ | BEST_EFFORT、RELIABLE と `max_blocking_time` |
| HISTORY | ✅ | KEEP_LAST、KEEP_ALL |
| RESOURCE_LIMITS | ✅ | |
| DURABILITY | ✅ | VOLATILE、TRANSIENT_LOCAL、TRANSIENT、PERSISTENT（#71）。TRANSIENT・PERSISTENT は writer の参加者が持つ。独立した durability service はない（[durability.md](durability.md)） |
| DURABILITY_SERVICE | ✅ | |
| DEADLINE | ✅ | |
| LIVELINESS | ✅ | AUTOMATIC、MANUAL_BY_PARTICIPANT、MANUAL_BY_TOPIC |
| WRITER_DATA_LIFECYCLE | ✅ | `autodispose_unregistered_instances` |
| READER_DATA_LIFECYCLE | ❌ | |
| OWNERSHIP、OWNERSHIP_STRENGTH | ❌ | すべて SHARED として振る舞う |
| DESTINATION_ORDER | ❌ | BY_RECEPTION_TIMESTAMP として振る舞う |
| LIFESPAN | ❌ | |
| LATENCY_BUDGET、TRANSPORT_PRIORITY | ❌ | |
| TIME_BASED_FILTER | ❌ | |
| PRESENTATION | ❌ | |
| ENTITY_FACTORY | ❌ | エンティティは作ったときに有効 |
| PROPERTY（参加者） | ✅ | DDS Security の `dds.sec.*` など（#69） |
| DATA_REPRESENTATION、TYPE_CONSISTENCY_ENFORCEMENT | ✅ | DDS-XTypes の QoS |

RxO（requested/offered）の比較は RELIABILITY、DURABILITY、DEADLINE、LIVELINESS について行い、合わなければ INCOMPATIBLE_QOS を両側に出す。

### 状態、listener、条件

| 項目 | 状況 | 備考 |
|---|---|---|
| 通信状態（INCONSISTENT_TOPIC、OFFERED/REQUESTED_DEADLINE_MISSED、OFFERED/REQUESTED_INCOMPATIBLE_QOS、SAMPLE_LOST、SAMPLE_REJECTED、LIVELINESS_LOST/CHANGED、PUBLICATION/SUBSCRIPTION_MATCHED、DATA_AVAILABLE、DATA_ON_READERS） | ✅ | |
| writer・reader・publisher・subscriber・topic・participant の listener と、上位への伝搬（2.2.4.3） | ✅ | #66 |
| StatusCondition、GuardCondition、ReadCondition、WaitSet | ✅ | |

### 適合プロファイル（DDS 1.4 の Conformance）

| プロファイル | 状況 |
|---|---|
| Minimum Profile | 🟡 上の ❌ の操作と QoS を除く |
| Content-Subscription Profile | 🟡 ContentFilteredTopic のみ。MultiTopic、QueryCondition はない |
| Persistence Profile | 🟡 TRANSIENT、PERSISTENT、DURABILITY_SERVICE はある。独立した durability service はない |
| Ownership Profile | ❌ |
| Object Model Profile | ❌（PRESENTATION の GROUP と coherent access がない） |

## DDSI-RTPS 2.5

`rustorb-rtps` にある。

| 項目 | 状況 | 備考 |
|---|---|---|
| メッセージと submessage の読み書き（8.3、9.4） | ✅ | 2.5 のすべての種類。未知・vendor 独自のものは読み飛ばす |
| 受信側の状態（INFO_SRC、INFO_DST、INFO_TS） | ✅ | |
| INFO_REPLY、INFO_REPLY_IP4 | 🟡 | 読むが、返信先の locator としては使わない |
| HEADER_EXTENSION（2.5 の新機能） | 🟡 | 読み飛ばすだけで、書かない |
| SPDP（8.5.3） | ✅ | マルチキャスト。ユニキャストだけの初期ピア一覧はない |
| SEDP（8.5.4） | ✅ | |
| Writer Liveliness Protocol（8.4.13） | ✅ | |
| best-effort と reliable の writer・reader（stateful） | ✅ | HEARTBEAT、ACKNACK、GAP |
| DATA_FRAG、HEARTBEAT_FRAG、NACK_FRAG（8.4.14.1） | ✅ | |
| keyed のトピック、key hash（`PID_KEY_HASH`）、`PID_STATUS_INFO` | ✅ | |
| `PID_CONTENT_FILTER_PROPERTY`、`PID_RELATED_SAMPLE_IDENTITY`、`PID_TYPE_INFORMATION`、`PID_DATA_REPRESENTATION` など | ✅ | |
| coherent set（`PID_COHERENT_SET`、`PID_GROUP_*`） | ❌ | PID の定数だけ |
| UDPv4 PSM とポート番号（9.6） | ✅ | |
| UDPv6 | ❌ | locator を読むだけで、送受信はしない |
| TCP | ❌ | 規格の外 |
| 共有メモリ | ✅ | rustorb-dds どうしの独自の転送。vendor の locator で告知（[shm.md](shm.md)） |
| Vendor ID | 🟡 | OMG から割り当てを受けていないので `VENDORID_UNKNOWN` を送る |

## DDS-XTypes 1.3

`rustorb-xtypes` にある。XCDR1・XCDR2 の符号化は rustorb の `rustorb-cdr` にある。

| 項目 | 状況 | 備考 |
|---|---|---|
| 型システム：プリミティブ（`int8`〜`float128`、`char`/`wchar`）、string/wstring、sequence、array、map、alias、enum、bitmask、struct（継承も）、union | ✅ | |
| bitset | 🟡 | TypeObject と DynamicData はある。規格に直列化がないので、ビットが収まる最小の符号なし整数で書く（rustorb 独自、Fast DDS とは通じない） |
| annotation 型（`@annotation`） | 🟡 | TypeObject と DynamicType。DynamicData の直列化はない |
| 再帰型（7.3.4.8）と SCC identifier（7.3.4.9） | ✅ | OpenDDS と同じハッシュになる（#70） |
| extensibility（FINAL、APPENDABLE、MUTABLE）、`@key`、`@optional`、`@id`、`@autoid`、`@hashid`、`@must_understand`、`@try_construct` | ✅ | |
| XCDR1、XCDR2（PLAIN、DELIMITED、PL）、DHEADER、EMHEADER | ✅ | encapsulation id は Table 60 で書き、Table 39 も読む |
| XML データ表現 | ❌ | |
| TypeObject（MINIMAL、COMPLETE）、TypeIdentifier、TypeInformation（7.3.4、7.6.3.2） | ✅ | |
| 型の表現：IDL、XML（7.3.2、7.3.3） | ❌ | TypeObject だけ |
| 型の assignability（7.2.4）と TYPE_CONSISTENCY_ENFORCEMENT（7.6.3.4） | ✅ | |
| TypeLookup service（7.6.3.3）：`getTypes`、`getTypeDependencies` | ✅ | OpenDDS は SCC identifier の getTypes に答えない（interop/README.md） |
| Dynamic Language Binding（7.5.2）：DynamicType、DynamicTypeBuilder、DynamicData | 🟡 | Rust と Python。C ABI には型を組み立てる関数はあるが、C++ と Java には DynamicData がない |
| 受け取った相手の型で読み書きする（`discover_topic_type`） | 🟡 | Rust と Python |
| Language Binding の Plain の型 | — | rustorb-idl が生成する |

## DDS Security 1.2

`rustorb-dds-security` にあり、`rustorb-dds` の feature `security`（既定では無効）で使う。詳しくは [security.md](security.md)。

| 項目 | 状況 | 備考 |
|---|---|---|
| 認証 DDS:Auth:PKI-DH | ✅ | X.509（EC P-256、RSA 2048）、DH MODP-2048-256、ECDH P-256 |
| 暗号化された秘密鍵、CRL、OCSP | ✅ | CRL と OCSP の property の名前は rustorb 独自（規格にない） |
| アクセス制御 DDS:Access:Permissions（governance、permissions、S/MIME 署名） | ✅ | |
| 暗号 DDS:Crypto:AES-GCM-GMAC（RTPS メッセージ、submessage、payload） | ✅ | ENCRYPT と SIGN |
| secure SEDP、secure WLP、DCPSParticipantsSecure（7.5.1.6） | ✅ | |
| Logging DDS:Logging:DDS_LogTopic（9.6、10.6） | 🟡 | ファイルと `DDS:Security:LogTopicV2`。LoggerListener、旧 `DDS:Security:LogTopic` はない |
| Data Tagging DDS:Tagging:DDS_Discovery（9.7） | 🟡 | Rust の QoS だけ |
| identity の失効、key revision | 🟡 | Rust だけ |
| PROPERTY QoS（`dds.sec.*`）による設定（7.2.5） | ✅ | 全言語 |
| `identity_status_token` の配布 | ❌ | 読み飛ばす |
| PSK（DDS:Auth:PSK） | ❌ | rustorb-dds#84 |
| PKCS #11（`pkcs11:` URI）、`https:` の OCSP responder | ❌ | |
| プラグインの差し替え（SPI を外から実装すること） | ❌ | 組み込みプラグインだけ |

## DDS-RPC 1.0

`rustorb_dds::rpc` にある。詳しくは [rpc.md](rpc.md)。

| 項目 | 状況 | 備考 |
|---|---|---|
| request/reply 形式：Requester、Replier、SampleIdentity | ✅ | 全言語 |
| Enhanced Service Mapping（`PID_RELATED_SAMPLE_IDENTITY`） | ✅ | |
| Basic Service Mapping（RequestHeader、ReplyHeader） | 🟡 | Rust だけ。interface からの `Call`/`Return` union は生成しない |
| サービスの discovery（7.6.2：`PID_RELATED_ENTITY_GUID`、`PID_SERVICE_INSTANCE_NAME`、`PID_TOPIC_ALIASES`） | ✅ | |
| 応答を求めた requester にだけ送る | ✅ | 他の reader には GAP |
| 関数呼び出し形式（`@DDSService` の interface から） | 🟡 | Rust の同期版だけ（rustorb-idl `-brust -Wbdds_only -Wbxtypes`）。`IAsync`、C++・Java・Python はない |
| `@DDSRequestTopic`、`@DDSReplyTopic` | ❌ | トピック名は設定で渡す |
| 規格の C++ binding（listener、future） | ❌ | 名前（`RequesterParams` など）だけ合わせた |

## ISO/IEC C++ PSM for DDS

`cpp/` のヘッダだけのライブラリ（C++17）。詳しくは [cpp.md](cpp.md)。

- ✅ DomainParticipant、Topic、ContentFilteredTopic、Publisher、Subscriber、DataWriter、DataReader（`select()`）、各 QoS ポリシー（上の DCPS の ✅ のもの）、状態と listener、条件と WaitSet、PSM の例外。
- ❌ 組み込みトピック、`find`、`ignore`、QueryCondition、coherent access、`set_qos`、DynamicData（`dds::core::xtypes`）。
- 🟡 型は rustorb-idl `-bc++11 -Wbdds_only` が書く。struct の継承、`map`、再帰型はまだない。

## Java 5 PSM for DDS

`java/` の Maven プロジェクト（Java 22 以降、FFM API）。詳しくは [java.md](java.md)。

- ✅ ServiceEnvironment、DomainParticipantFactory、DomainParticipant、Topic、ContentFilteredTopic、Publisher、Subscriber、DataWriter、DataReader（`select()`）、各 QoS ポリシー、状態と listener、条件と WaitSet、PSM の例外。
- ❌ 組み込みトピック、`find`、`ignore`、QueryCondition、coherent access、`setQos`、DynamicData。
- 🟡 型は rustorb-idl `-bjava -Wbdds_only` が書く。struct の継承、`map`、再帰型はまだない。Maven Central には公開していない。

## .NET と Ruby のバインディング

OMG に .NET と Ruby の DDS PSM はないので、.NET は Java 5 PSM の構造を C# に（[dotnet.md](dotnet.md)）、Ruby は C++ PSM の構造を Ruby に（[ruby.md](ruby.md)）移した。どちらも C ABI の上にある。

- ✅ DomainParticipant、Topic、ContentFilteredTopic、Publisher、Subscriber、DataWriter、DataReader、各 QoS ポリシー、状態と listener、条件と WaitSet。
- ❌ 組み込みトピック、`ignore`、QueryCondition、coherent access、`set_qos`、DynamicData、DDS-RPC。
- 🟡 型は rustorb-idl `-bcsharp -Wbdds_only`、`-bruby -Wbdds_only` が書く。struct の継承、`map`、再帰型、bitset はまだない。NuGet と RubyGems への公開は次のリリースから（#94）。

## IDL 4 と言語マッピング

IDL の生成器は [rustorb-idl](https://github.com/rustorb/rustorb-idl) にあり、Rust（`-brust`）、Python（`-bpython`）、C++11（`-bc++11`）、Java（`-bjava`）、C#（`-bcsharp`）、Ruby（`-bruby`）の DDS 用の型を書く（`-Wbdds` / `-Wbdds_only`）。Rust には OMG のマッピングがないので、rustorb 独自である。

- ✅ struct、union、enum、bitmask、typedef、sequence、array、string/wstring、module、XTypes の annotation（extensibility、`@key`、`@optional`、`@id` など）。`-Wbxtypes` で TypeObject も出す。
- 🟡 再帰型と bitset の DDS 出力は rustorb-idl#14。C++、Java、C#、Ruby では struct の継承と `map` がまだない。
- 🟡 構文解析器 idl_parser の制限：`switch(@key T)`、`sequence<@external T>` の annotation、`//@key` のコメント形式。

## 相互接続の確認状況

どれも黒箱として、配布されたプログラムと公開 API だけで試した。手順と、相手ごとに分かったことは [interop/README.md](../interop/README.md) にある。CI の `interop`、`opendds`、`ros2`、`openrtm` の各ジョブで毎回走る。

| 相手 | 版 | 確かめたこと |
|---|---|---|
| Cyclone DDS | 0.10.4 | best-effort、reliable（損失の修復）、TRANSIENT_LOCAL、TRANSIENT・PERSISTENT（rustorb-dds から）、LIVELINESS 3 種、DATA_FRAG、keyed（dispose、unregister）、ContentFilteredTopic（Cyclone の writer のみ）、XCDR2、型の進化と TypeLookup、DynamicData、DDS Security（RTPS 保護も） |
| Fast DDS | 2.11.2 | 同上。ただし TypeLookup と型の進化はない（Fast DDS に TypeLookup がない）。TRANSIENT は TRANSIENT_LOCAL の reader で |
| OpenDDS | 3.31 | best-effort、reliable、TRANSIENT_LOCAL、DATA_FRAG、keyed、XCDR2、型の進化と TypeLookup、再帰型（SCC）。Security、ContentFilter、LIVELINESS の試験はない |
| ROS 2 | Jazzy（rmw_fastrtps_cpp、rmw_cyclonedds_cpp） | `std_msgs/msg/String` の talker・listener、サービス `add_two_ints`、両方向 |
| OpenRTM-aist | 2.0.2（FastRTPSTransport、Fast DDS 2.6） | `RTC::TimedLong` の OutPort・InPort、Rust・C++・Python、両方向 |

言語ごとには、Rust、Python、C++、Java、.NET、Ruby の例がそれぞれ Cyclone DDS、Fast DDS、OpenDDS と試験される（Java は CI だけ）。

試験していない相互接続：DDS Security の OpenDDS、RTI Connext DDS（入手できない）、Cyclone DDS・Fast DDS の新しい版（0.11 以降、2.12 以降・3.x）、OpenRTM-aist-Python。

## 未対応のおもな項目

上の表の ❌ のうち、使う人が気づきやすいもの：

- DCPS：OWNERSHIP、DESTINATION_ORDER、LIFESPAN、TIME_BASED_FILTER、PRESENTATION と coherent access、READER_DATA_LIFECYCLE、作成後の QoS の変更、組み込みトピックの reader、`ignore_*`、QueryCondition、MultiTopic。
- RTPS：UDPv6、ユニキャストだけの discovery（初期ピア一覧）。
- XTypes：XML・IDL の型の表現、XML データ表現、C++・Java の DynamicData。
- Security：PSK（#84）、PKCS #11。
- RPC：非同期の関数呼び出し形式、他言語の関数呼び出し形式。
