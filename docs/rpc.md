# DDS-RPC（要求と応答）

rustorb/rustorb#111 の D5（#55）の 2 番目、#57 をまとめる。DDS-RPC 1.0 の request/reply 形式を、Rust の `rustorb_dds::rpc` と C ABI、C++、Java、Python で使える。ROS 2 のサービス（rmw_fastrtps_cpp）とも両方向に通じる。仕様は OMG の DDS-RPC 1.0 によった。ROS 2 は黒箱として扱い、配布されたプログラムを動かして、通信の中身を外から観察した。

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

- 応答は、答える要求の SampleIdentity（書き手の GUID と sequence number）を in-line QoS の `PID_RELATED_SAMPLE_IDENTITY`（0x0083）で運ぶ（7.8.2）。データ型に header は付けない（Enhanced Service Mapping、7.4.3）。
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

## 未対応

- 仕様の function-call 形式（インタフェースから生成するクライアントとサービス）と Basic Service Mapping（明示的な RequestHeader/ReplyHeader）。
- 応答トピックを content filter で絞ること（応答は読み手側で選ぶ）。
- `PID_TOPIC_ALIASES`（0x0082）は定義のみ。
