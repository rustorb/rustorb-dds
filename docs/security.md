# DDS Security

rustorb/rustorb#111 の D5（#55）の 4 番目、#59 をまとめる。OMG DDS Security 1.2（formal/25-03-06、wire は 1.1 と同じ）の組み込みプラグイン 3 つを、仕様から書いた。

- 認証 DDS:Auth:PKI-DH: X.509 証明書（EC P-256 または RSA 2048）と、DH（MODP-2048-256）または ECDH（P-256）の handshake。
- アクセス制御 DDS:Access:Permissions: permissions CA が S/MIME で署名した governance と permissions の XML 文書。
- 暗号 DDS:Crypto:AES-GCM-GMAC: AES-256-GCM（暗号化）と AES-256-GMAC（署名）。RTPS メッセージ、submessage、payload の各段で、governance の指定どおりに守る。

暗号には RustCrypto の crate（aes-gcm、sha2、hmac、p256、rsa、x509-cert、cms など）を、XML には roxmltree を使う。どれも MIT/Apache-2.0（`subtle` だけ BSD-3-Clause、RustCrypto が使う）。

## 使い方

`rustorb-dds` の feature `security`（既定では無効）で、`ParticipantConfig::security` に `SecurityConfig` を入れる。値は URI で、`file:` か `data:,`（後ろに文書そのもの）。

```rust
use rustorb_dds::{DomainParticipant, ParticipantConfig, SecurityConfig};

let mut config = ParticipantConfig::new(0);
config.security = Some(SecurityConfig {
    identity_ca: "file:/etc/dds/identity_ca.pem".into(),
    identity_certificate: "file:/etc/dds/cert.pem".into(),
    private_key: "file:/etc/dds/key.pem".into(),
    permissions_ca: "file:/etc/dds/permissions_ca.pem".into(),
    governance: "file:/etc/dds/governance.p7s".into(),
    permissions: "file:/etc/dds/permissions.p7s".into(),
    password: None,
    crl: None,
    logging: None,
});
let participant = DomainParticipant::with_config(config)?;
```

仕様の property 名（`dds.sec.auth.identity_ca` など）の組からは `SecurityConfig::from_properties`、または `ParticipantConfig::set_properties`（DomainParticipantQos の PROPERTY、7.2.5。`dds.sec.` で始まらない名前は無視し、6 つのうち欠けたものがあれば `Error::Security`。`rustorb.dds.sec.` で始まる名前も取る）。例 `hello_world` は、環境変数 `RUSTORB_DDS_SECURITY` で名前を指したファイル（1 行に `名前=値`）から読む。`RUSTORB_DDS_SECURITY_LOG=1` で、handshake の失敗などを stderr に出す。

### 暗号化された秘密鍵と CRL

#73 で足した。どちらも省ける。

- `dds.sec.auth.password`（`SecurityConfig::password`）: 暗号化された秘密鍵のパスワード。読める形は次のとおり。
  - PKCS #8 の `ENCRYPTED PRIVATE KEY`（PBES2。`openssl pkcs8 -topk8 -v2 aes-256-cbc` が書くもの）。
  - `Proc-Type: 4,ENCRYPTED` と `DEK-Info`（RFC 1421）の付いた `EC PRIVATE KEY`、`RSA PRIVATE KEY`、`PRIVATE KEY`。暗号は AES-128/192/256-CBC で、`openssl ec -aes256` などが書くもの。
- パスワードの扱い:
  - パスワードはパスフレーズとして使う。Cyclone DDS 0.10.4 で確かめたとおり、`openssl` で暗号化した鍵はそのまま読める。
  - 表 50 は「AES-128 の鍵そのものの Base64 で、AES128-CBC で復号する」と書く。そこで `DEK-Info` が AES-128-CBC のときは、パスフレーズとして復号できなければ、その読み方も試す。
  - property がなければ、`file:` の URI の問い合わせ部分の `password=`（表 50 の例 `file:key.pem?password=OpenSesame`）を使う。
- `rustorb.dds.sec.auth.crl`（`SecurityConfig::crl`）: identity CA の CRL（PEM の `X509 CRL` をいくつでも、または DER）の URI。この property は仕様に名前がないので、rustorb-dds 独自の名前にした（Cyclone DDS は `org.eclipse.cyclonedds.sec.auth.crl`）。
  - CRL は identity CA の署名があり、`thisUpdate` を過ぎていて、`nextUpdate` を過ぎていないものに限る。どれかを欠けば参加者を作れない。
  - 自分の証明書が失効していれば参加者を作れない（validate_local_identity）。handshake の相手の証明書が失効していれば、認証しない（validate_remote_identity、10.3.3）。
  - CRL は参加者を作るときに一度だけ読む。

### ログ（DDS:Logging:DDS_LogTopic）

#73 で足した、組み込みの logging plugin（9.6、10.6）。`SecurityConfig::logging` に `security_log::LogOptions` を入れるか、次の PROPERTY で設定する。名前は `dds.sec.<plugin>.builtin.<plugin 名>.` の形にした（仕様は名前を決めていない）。

- `dds.sec.log.builtin.DDS_LogTopic.logging_level`: この重さ以上を記録する。10.6 の `LoggingLevel` の名前（`ERROR_LEVEL`、`ERROR` でもよく、大文字小文字は問わない）か、番号（0 が EMERGENCY、7 が DEBUG）。既定は `ERROR_LEVEL`。
- `dds.sec.log.builtin.DDS_LogTopic.log_file`: 追記するファイル。
- `dds.sec.log.builtin.DDS_LogTopic.distribute`: `true` なら、トピック `DDS:Security:LogTopicV2`（型 `BuiltinLoggingTypeV2`、`rustorb_dds::security_log`）に publish する。

どれか一つでもあればログが有効になる。オプションは参加者を作るときに決まり、後から変えられない（9.6.2.1）。

- 記録する出来事と重さ:
  - 自分の証明書や permissions の検証の失敗（CRITICAL）。参加者を作れなかったときで、ファイルにだけ書く。
  - 参加者として domain に入ったこと（INFORMATIONAL）。
  - handshake の失敗（ERROR）と成功（INFORMATIONAL）。
  - 相手の permissions が domain に入ることを許さない（ERROR）。
  - 自分の writer・reader が permissions で拒まれた（ERROR）。
  - 相手の writer・reader が permissions で拒まれた（WARNING）。
- `structured_data` の `"DDS"` には `guid`、`domain_id`、`plugin_class`、`plugin_method` を入れる（表 47）。`msgid` は `<plugin_class>.<plugin_method>`、`facility` は 10。`hostname`（`HOSTNAME` か /etc/hostname）、`appname`（実行ファイル名）、`procid`（プロセス ID）も入れ、`hostip` は空にする。
- ファイルには 1 行に 1 件、`<RFC 3339 の UTC 時刻> <重さ> <plugin_class>.<plugin_method> guid=<GUID> domain_id=<id>: <本文>` の形で書く。
- トピックには 10.6 の組み込みの規則を、governance より先に当てる。
  - 書くのは domain に入れる参加者なら誰でもよい。
  - 読むには permissions の subscribe の grant が要る。
  - data は ENCRYPT、metadata は SIGN、discovery は守らない。
  - 1.1 までの名前 `DDS:Security:LogTopic` にも同じ規則を当てるが、publish するのは V2 だけ。
- writer は RELIABLE、KEEP_LAST 64 の VOLATILE。reader が match する前のメッセージは届かない。
- 型の `structured_data` は仕様では `map<string, NameValuePairSeq>`。rustorb-idl に map がないので、Rust の型では `StructuredDataEntry`（`key` と `value`）の列にしている。直列化は同じで、XTypes の型の記述（TypeObject）は map にしてある。
- `set_listener`（LoggerListener）はない。同じ参加者で読むときも grant が要る。
- `RUSTORB_DDS_SECURITY_LOG=1` は今までどおり、同じ出来事を stderr に出す。

### ほかの言語から

どれも DomainParticipantQos の PROPERTY に、上の 6 つの `dds.sec.*`（と、要れば `dds.sec.auth.password`、`rustorb.dds.sec.auth.crl`、`dds.sec.log.builtin.DDS_LogTopic.*`）を入れる。ログのトピックの型は今は Rust にだけある。C ABI、C++、Java のライブラリは feature `security` 付きで作る。

- C ABI: `rdds_participant_qos` の `property`（`rdds_property` の配列）と `property_len`。
- C++: `dds::core::policy::Property`（`set(name, value)`）を `DomainParticipantQos` に入れる。
- Java: `PolicyFactory.Property().with(name, value)` を `DomainParticipantQos` に入れる。
- Python: `DomainParticipant(domain_id, properties={...})`（dict か組の列）。

C++ と Python の例 `hello_world` も `RUSTORB_DDS_SECURITY` を読む。

## しくみ

- 参加者の GUID prefix は証明書の subject 名と候補の GUID から作る（10.3.3）。
- SPDP で identity token、permissions token、`PID_PARTICIPANT_SECURITY_INFO` を告知する。identity token を告知する相手とは、stateless message の endpoint（best-effort）で handshake をし、通るまでその相手との discovery を進めない。governance が `allow_unauthenticated_participants` を許すときは、認証しない相手には守られていないものだけを見せる。
- handshake のあと、相手の permissions を CA で確かめて、domain に入れるか（`enable_join_access_control`）を決める。鍵は volatile message secure endpoint（reliable、KxKey で暗号化）で交わす。
- 守られた topic の writer と reader は、相手の permissions が publish か subscribe を許すときだけ match する（`enable_read_access_control`、`enable_write_access_control`）。
- `enable_discovery_protection` の topic の endpoint は secure SEDP（`DCPSPublicationsSecure` など）で、`enable_liveliness_protection` なら secure WLP で告知する。これらの endpoint にも鍵がある（9.8.9.3）。
- `enable_discovery_protection` のとき、認証の済んだ相手とは secure SPDP（`DCPSParticipantsSecure`、7.5.1.6）でも参加者のデータを交わす（reliable、transient local、KEEP_LAST 1）。参加者の削除もそこで送り、相手の ack を最大 300 ms 待つ。secure SPDP でデータを受け取った相手については、平文 SPDP からはデータの変更も dispose も受け取らず、lease の更新だけに使う。
- SPDP と stateless、volatile のメッセージは bootstrap なので RTPS protection の外に置く（7.5.7）。

## 確かめたこと

- `tests/security.rs`（守られた topic の送受信、permissions による拒否、認証しない参加者の無視、secure SPDP での削除）、暗号化された秘密鍵と、CRL で失効した相手の拒否、ログのファイルとトピック（grant のある参加者だけが読める）（#73）、C ABI・C++・Java・Python の PROPERTY からの設定の試験と `rustorb-dds-security` の単体試験（`tests/revocation.rs`: 鍵の形式ごとの復号、誤ったパスワード、期限切れや別の CA の CRL）。
- 守られた topic の pcap に平文の標本がなく、メッセージが SRTPS で守られていること。
- `interop/run.sh` の `-secure` の 16 ケース: Cyclone DDS 0.10.4、Fast DDS 2.11.2 と 2 方向で、best-effort（discovery、liveliness、metadata、data を暗号化）と、RTPS メッセージ全体も暗号化した reliable。Rust の例に加えて、Python（best-effort）と C++（reliable）の例が PROPERTY で設定したもの（#69）。RTPS protection SIGN も手で試して通った。#73 からは、rustorb 側の秘密鍵を暗号化し（`dds.sec.auth.password`）、別の証明書を失効させた CRL を与えている。相手ごとに合わせたことは `interop/README.md` にある。

## 未対応

- secure SPDP の `identity_status_token`（読み飛ばすだけで、送らない）。
- OCSP、PKCS #11（`pkcs11:` の URI）、PSK（DDS:Auth:PSK）、data tagging。
- ログの LoggerListener、レガシーの `DDS:Security:LogTopic` への publish、他の言語でのログのトピックの型。
- 鍵の更新（key revision）。
