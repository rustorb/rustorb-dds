# DDS-RPC（要求と応答）

rustorb/rustorb#111 の D5（#55）の 2 番目、#57 をまとめる。DDS-RPC 1.0 の request/reply 形式を、Rust の `rustorb_dds::rpc` と C ABI、C++、Java、Python で使える。ROS 2 のサービス（rmw_fastrtps_cpp）とも両方向に通じる。仕様は OMG の DDS-RPC 1.0 によった。ROS 2 は黒箱として扱い、配布されたプログラムを動かして、通信の中身を外から観察した。#72 で残り（関数呼び出し形式、Basic Service Mapping、トピックの別名、応答を求めた requester にだけ送ること）を足した。

## 使い方

`Requester<TReq, TRep>` は要求を要求トピックに書き、その応答を応答トピックで受ける。`Replier<TReq, TRep>` は要求を受け、それぞれに応答を返す。

```rust
use rustorb_dds::rpc::{Replier, Requester};

let replier = Replier::<Req, Rep>::new(&participant, "Calc")?;      // Calc_Request / Calc_Reply
let requester = Requester::<Req, Rep>::new(&participant, "Calc")?;
requester.wait_for_service(timeout);

// サービス側
for r in replier.receive_requests(timeout)? {
    replier.send_reply(&answer(&r.data), &r.id)?;
}

// 要求側
let id = requester.send_request(&req)?;      // 要求の SampleIdentity
let rep = requester.receive_reply(&id, timeout)?;
// あるいは requester.request(&req, timeout)?
```

- トピック名は既定で `<service>_Request` と `<service>_Reply`（7.4.1）。`RpcConfig::with_topics` で任意の名前にできる（ROS 2 は `rq/<name>Request` と `rr/<name>Reply`）。
- QoS は既定で両側とも RELIABLE、KEEP_ALL。`RpcConfig` の `writer_qos`、`reader_qos` で変える。`service_instance_name` は replier だけが告知する。
- `with_topics` は任意の標本型のトピックから作る（`DynamicData` も）。各言語の binding はこれを使う。
- 1 つの requester の要求に、複数の replier が答えてもよい。`take_replies` はある要求への応答をすべて取る。

各言語では次のとおり。

| | 要求側 | サービス側 |
|---|---|---|
| C | `rdds_requester_new`、`_wait_for_service`、`_send_request`、`_receive_reply` | `rdds_replier_new`、`_receive_request`（1 件ずつ）、`_send_reply` |
| C++ | `dds::rpc::Requester<TReq, TRep>(RequesterParams)` | `dds::rpc::Replier<TReq, TRep>(ReplierParams)` |
| Java | `org.rustorb.dds.Requester<TReq, TRep>(requestTopic, replyTopic)` | `org.rustorb.dds.Replier` |
| Python | `rustorb_dds.Requester(participant, request_topic, reply_topic)` | `rustorb_dds.Replier` |

C++ の `RequesterParams`/`ReplierParams` は仕様の C++ binding の名前に合わせたが、仕様の C++ ヘッダ全体（listener、future、function-call 形式）は実装していない。Java の DDS-RPC binding は仕様の外部ファイルで定義されていて手元にないので、rustorb の API として `org.rustorb.dds` に置いた。

## 応答と要求の対応

- 応答は、答える要求の SampleIdentity（書き手の GUID と sequence number）を in-line QoS の `PID_RELATED_SAMPLE_IDENTITY`（0x0083）で運ぶ（7.8.2）。データ型に header は付けない（Enhanced Service Mapping、7.4.3）。header を付ける Basic Service Mapping は後述。
- `SampleInfo::related_sample_identity` で受け手もこれを読める。`DataWriter::write_w_params` で任意の書き込みに付けられる（`WriteParams`）。
- 要求も `PID_RELATED_SAMPLE_IDENTITY` を運び、要求側の応答 reader の GUID と `SEQUENCENUMBER_UNKNOWN` を入れる。replier は、要求がこれを持てばその GUID と要求の sequence number を、持たなければ要求の書き手と sequence number を応答に入れる。requester はどちらの形の応答も自分の要求のものと認める。これは ROS 2（rmw_fastrtps_cpp）がそうしていることを観察したためで、仕様（7.8.2 は応答に使うと定める）の外である。仕様どおりの相手はこれを無視するので害はない。

## サービスの discovery（7.6.2）

- requester と replier は、それぞれの writer と reader を組として SEDP で告知する。`PID_RELATED_ENTITY_GUID`（0x0081）で相手方の GUID を、replier は `PID_SERVICE_INSTANCE_NAME`（0x0080）も入れる。
- `Requester::wait_for_service` は、要求 writer が合致した要求 reader と、応答 reader が合致した応答 writer が組である replier が見つかるまで待つ（7.6.2.2.2）。
- `Replier` は、ある requester の応答 reader と合致するまで、その requester の要求を渡さずに保持する（7.6.2.2.1）。合致すれば渡す。
- 相手の endpoint が `PID_RELATED_ENTITY_GUID` を告知しない場合（ROS 2 など）は、同じ participant の endpoint どうしを組とみなす。

## ROS 2 のサービス

例 `ros2_service`（`crates/rustorb-dds/examples/ros2_service`）は `demo_nodes_cpp` の `add_two_ints_server`、`add_two_ints_client` と同じことをする。

```
cargo run --example ros2_service -- server [domain [count [timeout_s]]]
cargo run --example ros2_service -- client [domain [count [timeout_s]]]
```

外から観察した ROS 2 Jazzy（rmw_fastrtps_cpp 8.4、Fast DDS 2.14）のサービスの姿は次のとおり。

- サービス `/name` は要求トピック `rq/nameRequest` と応答トピック `rr/nameReply`。型 `pkg/srv/Name` は `pkg::srv::dds_::Name_Request_` と `pkg::srv::dds_::Name_Response_`（`interop/idl/AddTwoInts.idl`）。
- QoS は RELIABLE、VOLATILE、KEEP_LAST(10)。endpoint の USER_DATA に型の `typehash=RIHS01_...;` がある。
- 要求の in-line QoS には `PID_RELATED_SAMPLE_IDENTITY` と、同じ値の vendor 固有の PID 0x800f があり、値はクライアントの応答 reader の GUID と `SEQUENCENUMBER_UNKNOWN`（high -1、low 0）。
- 応答の in-line QoS の `PID_RELATED_SAMPLE_IDENTITY`（と 0x800f）は、クライアントの応答 reader の GUID と要求の sequence number。
- SEDP に `PID_RELATED_ENTITY_GUID` はない。

`interop/ros2.sh` は、ROS 2 の client と rustorb-dds の server、rustorb-dds の client と ROS 2 の server の 2 方向を、各 rmw で試す（`-client-to-rustorb`、`rustorb-client-to-`）。

| 日付 | ROS 2 | 結果 |
|---|---|---|
| 2026-11-10 | Jazzy、rmw_fastrtps_cpp（`osrf/ros:jazzy-desktop`） | 2 方向とも通過 |

rmw_cyclonedds_cpp は手元の環境（packages.ros.org に届かない）で動かせないので、CI の job `ros2` で試す。

CI の job `ros2` の pcap から見た rmw_cyclonedds_cpp（Cyclone DDS 0.10、vendor id 0x0110）のサービスは、Fast DDS と違う。

- 要求に in-line QoS がなく、payload の encapsulation と本体のあいだに 16 octet のヘッダがある。クライアントの 8 octet の id と、クライアント自身の 64 bit の sequence number（encapsulation の byte order）。
- 応答はそのヘッダを頭に付けて返す。
- クライアントの要求 writer と応答 reader は USER_DATA に `clientid= 1.10.32.b4....0.0.0.8;`（16 octet を 16 進で `.` 区切り）を載せ、サービスは要求の writer と同じ `clientid` の reader と match するまで応答しない（`Requester` も自分の writer の GUID で載せる）。サービス側は `serviceid= ...;` を載せる。
- Cyclone DDS は in-line QoS があって `PID_KEY_HASH` がない DATA を捨てる（key のない topic でも）。key のない writer は in-line QoS を送るときは常に key hash（ゼロ）を付ける。

`Requester` は、Cyclone DDS の参加者の要求 reader と match していればヘッダを付けて送り、Cyclone DDS の writer からの応答はヘッダで要求を見分ける。`Replier` は Cyclone DDS の writer からの要求のヘッダを外して覚え、その要求への応答に付ける。

## 応答は求めた requester にだけ（#72）

- `Replier::send_reply` は、応答を求めた requester の応答 reader にだけ送る。要求が名指す reader（rustorb-dds と ROS 2 rmw_fastrtps_cpp の要求は応答 reader を名指す）が match していればそれに、そうでなければ要求の writer と組の reader が一つだけならそれに送る。ほかの reader には、その sequence number について GAP を送る（writer 側の filter で落とした標本と同じ扱い、DDSI-RTPS 2.5、8.7.3.4）。同じ participant の reader にも渡さない。どの reader か決まらなければ、全員に送る。
- 7.2.1 の「client 側の reader の content filter」の役目を writer 側で果たす。Enhanced Service Mapping では要求の identity がデータの中にないので、標準の filter 式では書けないため。
- 任意の書き込みにも `WriteParams::directed_to` で使える。rustorb-rtps では `ReliableWriter::direct`。

## トピックの別名（#72）

- `RpcConfig::request_topic_aliases` と `reply_topic_aliases` を SEDP の `PID_TOPIC_ALIASES`（0x0082、7.4.3、7.6.2.1）で告知する。受け取った別名も読む。
- writer と reader は、トピック名が同じか、一方の名前が他方の別名にあれば match する（`rustorb_rtps::sedp::same_topic`）。型の名前は違ってよく、型の整合は XTypes の assignability で見る（両方が TypeInformation を告知していれば）。
- 継承したインタフェースのサービスは、基底のトピック名を別名に持つ。基底のクライアントは自分のトピック名で探して、派生のサービスと match する。

## Basic Service Mapping（#72）

- `rustorb_dds::rpc::idl` に 7.5.1.1.1 の共通の型がある（`src/rpc/DdsRpc.idl` を rustorb-idl で生成した）。`RequestHeader`（`request_id`、`instance_name`）、`ReplyHeader`（`related_request_id`、`remote_ex`）、`RemoteExceptionCode`。rustorb_rtps の `SampleIdentity` と `From` で行き来できる。生成コードからは `-Wbextern=dds=::rustorb_dds::rpc::idl::dds` で参照する。
- 要求と応答の型は、`header` を持つ struct（型の組、7.3.1.4）で、`BasicRequest`、`BasicReply` を実装する。rustorb-idl は `@RPCRequestType`、`@RPCReplyType` の struct にこれを実装する。
- `Requester::basic(participant, config)` は、要求ごとに header の `request_id` に writer の GUID と要求の sequence number を、`instance_name` に `config.service_instance_name`（なければ空）を入れる。応答は header の `related_request_id` で見分ける。応答 reader は応答トピックの content filtered topic で、`header.relatedRequestId.writer_guid` が自分の要求 writer のものだけを通す（SEDP で filter を告知するので、writer 側で filter する相手なら送られもしない）。
- `Replier::basic(participant, config)` は header の `request_id` で要求を知り、`send_reply` で応答の header の `related_request_id` に入れる（`remote_ex` は渡された応答のまま）。`service_instance_name` があれば、ほかの instance 宛ての要求は捨てる（空の instance 名はどれ宛てでもある）。
- 応答にも `PID_RELATED_SAMPLE_IDENTITY` を付ける。Basic の相手はこれを見ない。
- Basic Service Mapping の discovery（7.6.1）には `PID_RELATED_ENTITY_GUID` などの拡張がないが、rustorb-dds どうしでは告知するので、`wait_for_service` の組の確認も効く。相手が告知しなければ、同じ participant の endpoint を組とみなす（Enhanced と同じ）。

## 関数呼び出し形式（#72）

rustorb-idl の `-brust -Wbdds_only -Wbxtypes` は、`@DDSService` のインタフェースに、Enhanced Service Mapping の型（7.5.1.2）と Rust のクライアントとサーバを書く（rustorb-idl の docs/dds-rpc.md）。

```rust
impl Adder for Service {
    fn add(&self, a: i32, b: i32) -> Result<i32, RpcError<AdderAddException>> {
        Ok(a + b)
    }
}
let server = CalculatorServer::new(&participant, Service)?;   // Calc_Calculator_Service_Request / _Reply
std::thread::spawn(move || server.run(&stop));
let client = CalculatorClient::new(&participant)?;
client.wait_for_service(timeout);
let sum = client.add(2, 3)?;
```

- インタフェース `I` は trait `I`（基底の trait が supertrait）、`IClient`、`IServer<S: I>` になる。メソッドは in と inout の引数を取り、戻り値（out か inout があれば Out struct）か `RpcError<E>` を返す。`E` は操作の例外の enum（なければ `Infallible`）。
- `RpcError` は例外、相手の remote exception code、応答の形が合わない、ローカルのエラー（`Error::Timeout` など）。サーバは例外を Result に、`RpcError::Remote(code)` を `remoteEx` に、それ以外を `REMOTE_EX_UNKNOWN_EXCEPTION` にして返す。
- トピックは `<モジュール>_<I>_Service_Request`、`_Reply`（7.4.1）。サーバは基底の名前を別名に持つ。
- `crates/rustorb-dds/tests/rpc_service.rs`：派生インタフェースのサーバに、派生と基底のクライアントが呼ぶ。例外、remote exception、out・inout、one-way、属性、サービス名。

## 未対応

- インタフェースからの Basic Service Mapping の型（`Call`、`Return` の union）。Basic のサービスは型の組で書ける。
- 関数呼び出し形式の非同期版（`IAsync`）と、C++、Java、Python の関数呼び出し形式。C ABI、C++、Java、Python の binding は request/reply 形式のまま（別名と Basic は Rust だけ）。
- `@DDSRequestTopic`、`@DDSReplyTopic`（トピック名は `with_config` で渡せる）。
