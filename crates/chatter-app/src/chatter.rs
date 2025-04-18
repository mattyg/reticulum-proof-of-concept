use crate::types::AnnounceInfo;
use rand_core::OsRng;
use reticulum::{
    destination::{
        link::{self, Link, LinkEventData}, DestinationName, SingleInputDestination, SingleOutputDestination
    },
    hash::AddressHash,
    identity::PrivateIdentity,
    iface::tcp_client::TcpClient,
    transport::{Transport, TransportConfig},
};
use rmp_serde::Serializer;
use serde::Serialize;
use std::{collections::{HashMap, HashSet}, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, broadcast::Receiver},
    task::AbortHandle,
    time,
};

pub struct ChatterTransport {
    transport: Transport,

    /// Others who are chatting about the same topic
    peers: Mutex<HashSet<AddressHash>>,
}

pub type ChatterTransportHandle = Arc<ChatterTransport>;

/// A reticulum node that can chat
pub struct Chatter {
    transport_handle: ChatterTransportHandle,
    announce_task: AbortHandle,
    recv_announce_task: AbortHandle,
    recv_in_link_event_task: AbortHandle,
}

impl Drop for Chatter {
    fn drop(&mut self) {
        self.announce_task.abort();
        self.recv_announce_task.abort();
        self.recv_in_link_event_task.abort();
    }
}

impl Chatter {
    pub async fn new(server_host: String, nick: String, topic: String) -> Self {
        // Create Identity
        let identity = PrivateIdentity::new_from_rand(OsRng);

        // Create the Transport
        let mut transport = Transport::new(TransportConfig::new(nick.clone(), false));

        // Add the Tcp client interfaces
        transport.iface_manager().lock().await.spawn(
            TcpClient::new(server_host),
            TcpClient::spawn,
        );

        // Create my incoming Destination to chat on the topic
        let destination_name = DestinationName::new("chatter", format!("chat.{}", topic).as_str());
        println!("Crated my destination {}", destination_name.hash);
        let in_destination = transport.add_destination(identity, destination_name).await;

        let transport_handle = Arc::new(ChatterTransport {
            transport,
            peers: Mutex::new(HashSet::new())
        });

        // Start the outgoing announce task
        let announce_info = AnnounceInfo { nick };
        let announce_task = tokio::task::spawn(announce_task(
            transport_handle.clone(),
            in_destination.clone(),
            announce_info,
        ))
        .abort_handle();

        // Handle incoming announce packets
        let recv_announce_task = tokio::task::spawn(recv_announce_task(
            transport_handle.clone(),
            destination_name,
            in_destination.clone(),
            transport_handle.transport.recv_announces().await,
        ))
        .abort_handle();

        // Handle incoming link events
        let recv_in_link_event_task = tokio::task::spawn(recv_in_link_event_task(
            transport_handle.clone(),
            transport_handle.transport.in_link_events(),
        ))
        .abort_handle();

        Self {
            transport_handle,
            announce_task,
            recv_announce_task,
            recv_in_link_event_task,
        }
    }

    /// Send a chat message
    pub async fn chat(&self, message: &[u8]) {
        let peers = self.transport_handle.peers.lock().await;
        
        if peers.is_empty() {
            println!("Chat message sent, but no peers in peer_store. Ignoring...");
        }
        
        // Send message to all my peers
        for address_hash in peers.iter() {
            println!("Sending message to address {}", address_hash);
            self.transport_handle.transport.send_to_out_links(address_hash, message).await;
        }
    }
}

/// Task that loops forever, sending my own announce packet every `config.anounce_interval_ms`
async fn announce_task(
    transport_handle: ChatterTransportHandle,
    in_destination: Arc<Mutex<SingleInputDestination>>,
    announce_info: AnnounceInfo,
) {
    let _ = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(5000));

        // Serialize AnnounceInfo to msgpack
        let mut buf = Vec::new();
        announce_info
            .clone()
            .serialize(&mut Serializer::new(&mut buf))
            .unwrap();
        let announce_info_bytes_slice = buf.as_slice();

        loop {
            interval.tick().await;

            let packet = in_destination
                .clone()
                .lock()
                .await
                .announce(OsRng, Some(announce_info_bytes_slice))
                .unwrap();
            let _ = transport_handle.transport.send_broadcast(packet).await;
            println!(
                "\n***\nAnnounced myself as '{}' {}\n***\n",
                announce_info.nick,
                in_destination.clone().lock().await.identity.address_hash()
            );
        }
    })
    .await;
}

/// Task that awaits receiving announce packets from others, and adds them to my FriendStore
async fn recv_announce_task(
    transport_handle: ChatterTransportHandle,
    destination_name: DestinationName,
    in_destination: Arc<Mutex<SingleInputDestination>>,
    mut recv: Receiver<Arc<Mutex<SingleOutputDestination>>>,
) {
    while let Ok(destination) = recv.recv().await {
        // Check if announce is for the same destination name
        let destination = destination.lock().await;
        if destination.desc.name.as_name_hash_slice() == destination_name.as_name_hash_slice() {
            println!("Received announce for my destination name");
            {
                let in_destination_lock = in_destination.lock().await;
                let mut peers_lock = transport_handle.peers.lock().await;

                // Check if announce is already in the peer store, or is for my own identity
                if !peers_lock.contains(&destination.identity.address_hash)
                    && &destination.identity.address_hash
                        != in_destination_lock.identity.address_hash()
                {
                   // Create a new Link with this peer
                    print!("Adding link with desc address={} identiy address ={}", destination.desc.address_hash, destination.identity.address_hash);
                    let _ = transport_handle.transport.link(destination.desc).await;
                    // Add peer & Link to peers store
                    peers_lock.insert(destination.desc.address_hash);

                    println!(
                        "Updated peers store: {:?}\n\n",
                        peers_lock
                            .iter()
                            .map(|p| format!("{}", p))
                            .collect::<Vec<String>>()
                    );
                } else {
                    println!("Already in peers store, ignoring...");
                }
            }
        } else {
            println!("Different destination name, ignoring...");
        }
    }
}

async fn recv_in_link_event_task(
    transport_handle: ChatterTransportHandle,
    mut recv: Receiver<LinkEventData>,
) {
    while let Ok(link_event_data) = recv.recv().await {
        match link_event_data.event {
            link::LinkEvent::Activated => {
                print!("Received Link Activated");
            }
            link::LinkEvent::Closed => {
                print!("Received Link Closed");
                let mut peers_lock = transport_handle.peers.lock().await;
                peers_lock.remove(&link_event_data.address_hash);
            }
            link::LinkEvent::Data(payload) => {
                print!(
                    "<< {}",
                    String::from_utf8(payload.as_slice().to_vec()).unwrap()
                );
            }
        }
    }
}
