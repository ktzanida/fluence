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

use std::{error::Error, ops::DerefMut, time::Duration};

use async_std::{task, task::JoinHandle};
use derivative::Derivative;
use futures::{
    channel::{mpsc, mpsc::TrySendError, oneshot},
    future::FusedFuture,
    select,
    stream::{Fuse, StreamExt},
    FutureExt,
};
use libp2p::core::either::EitherError;
use libp2p::core::Multiaddr;
use libp2p::swarm::{ProtocolsHandlerUpgrErr, SwarmEvent};
use libp2p::{identity::Keypair, PeerId, Swarm};

use fluence_libp2p::{
    build_memory_transport, build_transport,
    types::{Inlet, OneshotOutlet, Outlet},
    Transport,
};
use particle_protocol::{Particle, ProtocolConfig};

use crate::api::ParticleApi;
use crate::{behaviour::ClientBehaviour, ClientEvent};

#[derive(Debug)]
struct Command {
    node: PeerId,
    particle: Particle,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Client {
    #[derivative(Debug = "ignore")]
    pub key_pair: Keypair,
    pub peer_id: PeerId,
    /// Channel to send commands to node
    relay_outlet: Outlet<Command>,
    /// Stream of messages received from node
    client_inlet: Fuse<Inlet<ClientEvent>>,
    stop_outlet: OneshotOutlet<()>,
}

impl Client {
    fn new(
        relay_outlet: Outlet<Command>,
        client_inlet: Inlet<ClientEvent>,
        stop_outlet: OneshotOutlet<()>,
        key_pair: Option<Keypair>,
    ) -> Self {
        let key = key_pair.unwrap_or_else(Keypair::generate_ed25519);
        let peer_id = key.public().to_peer_id();

        Client {
            key_pair: key,
            peer_id,
            relay_outlet,
            client_inlet: client_inlet.fuse(),
            stop_outlet,
        }
    }

    pub fn send(&self, particle: Particle, node: PeerId) {
        if let Err(err) = self.relay_outlet.unbounded_send(Command { node, particle }) {
            let err_msg = format!("{:?}", err);
            let msg = err.into_inner();
            log::warn!("Unable to send msg {:?}: {:?}", msg, err_msg)
        }
    }

    pub fn receive_one(&mut self) -> impl FusedFuture<Output = Option<ClientEvent>> + '_ {
        self.client_inlet.next()
    }

    pub fn stop(self) {
        if self.stop_outlet.send(()).is_err() {
            log::warn!("Unable to send stop, channel closed")
        }
    }

    pub fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        self.key_pair.sign(bytes).expect("signing error")
    }

    fn dial(
        &self,
        node: Multiaddr,
        transport: Transport,
        transport_timeout: Duration,
        protocol_config: ProtocolConfig,
    ) -> Result<Swarm<ClientBehaviour>, Box<dyn Error>> {
        let mut swarm = {
            let behaviour = ClientBehaviour::new(protocol_config);

            let transport = build_transport(transport, self.key_pair.clone(), transport_timeout);
            Swarm::new(transport, behaviour, self.peer_id)
        };

        match Swarm::dial(&mut swarm, node.clone()) {
            Ok(_) => log::info!("{} dialed to {:?}", self.peer_id, node),
            Err(e) => {
                log::error!("Dial to {:?} failed with {:?}", node, e);
                return Err(e.into());
            }
        }

        Ok(swarm)
    }

    pub async fn connect(
        relay: Multiaddr,
        transport_timeout: Duration,
    ) -> Result<(Client, JoinHandle<()>), Box<dyn Error>> {
        Self::connect_with(relay, Transport::Network, None, transport_timeout).await
    }

    pub async fn connect_with(
        relay: Multiaddr,
        transport: Transport,
        key_pair: Option<Keypair>,
        transport_timeout: Duration,
    ) -> Result<(Client, JoinHandle<()>), Box<dyn Error>> {
        let (client_outlet, client_inlet) = mpsc::unbounded();
        let (relay_outlet, relay_inlet) = mpsc::unbounded();

        let (stop_outlet, stop_inlet) = oneshot::channel();

        let protocol_config = ProtocolConfig::new(
            transport_timeout,
            // keep alive timeout
            Duration::from_secs(10),
            transport_timeout,
        );
        let client = Client::new(relay_outlet, client_inlet, stop_outlet, key_pair);
        let mut swarm = client.dial(relay, transport, transport_timeout, protocol_config)?;

        let mut relay_inlet = relay_inlet.fuse();
        let mut stop = stop_inlet.into_stream().fuse();

        let task = task::spawn(async move {
            loop {
                select!(
                    // Messages that were scheduled via client.send() method
                    to_relay = relay_inlet.next() => {
                        if let Some(cmd) = to_relay {
                            Self::send_to_node(swarm.behaviour_mut(), cmd)
                        }
                    }

                    // Messages that were received from relay node
                    from_relay = swarm.select_next_some() => {
                        match Self::receive_from_node(from_relay, &client_outlet) {
                            Err(err) => {
                                let err_msg = format!("{:?}", err);
                                let msg = err.into_inner();
                                log::warn!("unable to send {:?} to node: {:?}", msg, err_msg)
                            },
                            Ok(_v) => {},
                        }
                    },
                    _ = stop.next() => break,
                )
            }
        });

        // TODO: return client only when connection is established, i.e. wait for an event from swarm
        Ok((client, task))
    }

    fn send_to_node<R: ParticleApi>(swarm: &mut R, cmd: Command) {
        let Command { node, particle } = cmd;
        swarm.send(node, particle)
    }

    fn receive_from_node(
        msg: SwarmEvent<
            ClientEvent,
            EitherError<ProtocolsHandlerUpgrErr<std::io::Error>, libp2p::ping::Failure>,
        >,
        client_outlet: &Outlet<ClientEvent>,
    ) -> Result<(), TrySendError<ClientEvent>> {
        if let SwarmEvent::Behaviour(msg) = msg {
            // Message will be available through client.receive_one
            client_outlet.unbounded_send(msg)
        } else {
            Ok(())
        }
    }
}
