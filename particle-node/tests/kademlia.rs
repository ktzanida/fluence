/*
 * Copyright 2020 Fluence Labs Limited
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use connected_client::ConnectedClient;
use created_swarm::make_swarms;
use test_constants::KAD_TIMEOUT;

use eyre::WrapErr;
use libp2p::PeerId;
use maplit::hashmap;
use serde_json::{json, Value as JValue};
use std::thread::sleep;

#[test]
fn neighborhood() {
    let swarms = make_swarms(3);
    sleep(KAD_TIMEOUT);
    let mut client = ConnectedClient::connect_to(swarms[0].multiaddr.clone())
        .wrap_err("connect client")
        .unwrap();

    client.send_particle(
        r#"
            (seq
                (call node ("kad" "neighborhood") [node] peers)
                (call client ("return" "") [peers] void)
            )
        "#,
        hashmap! {
            "node" => json!(client.node.to_string()),
            "client" => json!(client.peer_id.to_string())
        },
    );
    let response = client.receive_args().wrap_err("receive").unwrap();
    if let JValue::Array(neighborhood) = response[0].clone() {
        assert_eq!(neighborhood.len(), 2);

        let assert_contains = |id: &PeerId| {
            assert!(neighborhood
                .iter()
                .find(|v| **v == JValue::String(id.to_string()))
                .is_some())
        };

        assert_contains(&swarms[1].peer_id);
        assert_contains(&swarms[2].peer_id);
    } else {
        panic!("response[0] must be an array, response was {:#?}", response);
    }
}
