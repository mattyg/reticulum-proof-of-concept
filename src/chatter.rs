use crate::types::{AnnounceInfo, Message};
use std::{collections::{BTreeSet, HashMap, HashSet}, io::Cursor, sync::Arc, time::Duration};

use reticulum::{buffer::StaticBuffer, destination::{link::{self, Link, LinkEventData}, Destination, DestinationName, SingleInputDestination, SingleOutputDestination}, hash::AddressHash, identity::{Identity, PrivateIdentity}, iface::{tcp_client::TcpClient, tcp_server::TcpServer}, packet::Packet, transport::Transport};
use rmp_serde::{Deserializer, Serializer};
use serde::{Serialize, Deserialize};
use tokio::{sync::{broadcast::{self, Receiver}, Mutex}, task::AbortHandle, time};
use rand_core::OsRng;

/// A reticulum node that can chat
pub struct Chatter {
    transport: Transport,

    /// Others who are chatting about the same topic
    peers: HashMap<AddressHash, Arc<Mutex<Link>>>,
    
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
    pub async fn new(nick: String, topic: String) -> Self {
        // Create Identity
        let identity = PrivateIdentity::new_from_rand(OsRng);
        
        // Create empty peers list
        let peers = HashMap::new();

        // Create the Transport
        let mut transport = Transport::new();

        // Add the Tcp client interfaces
        transport
            .iface_manager()
            .lock()
            .await
            .spawn(TcpClient::new("reticulum.betweentheborders.com:4242"), TcpClient::spawn);

        // Create my incoming Destination to chat on the topic
        let destination_name = DestinationName::new("chatter", format!("chat.{}", topic).as_str());
        println!("Crated my destination {}", destination_name.hash);
        let in_destination = transport.add_destination(identity, destination_name).await;
        
        // Start the outgoing announce task 
        let announce_info = AnnounceInfo { nick };
        let announce_task = tokio::task::spawn(announce_task(transport.clone(), in_destination.clone(), announce_info))
            .abort_handle();

        // Handle incoming announce packets
        let recv_announce_task = tokio::task::spawn(recv_announce_task(transport.clone(), peers.clone(), destination_name, in_destination.clone(), transport.clone().recv_announces().await))
            .abort_handle();

        // Handle incoming link events
        let recv_in_link_event_task = tokio::task::spawn(recv_in_link_event_task(peers.clone(), transport.clone().in_link_events()))
            .abort_handle();



        // Handle incoming link events
        let recv_out_link_event_task = tokio::task::spawn(recv_out_link_event_task(peers.clone(), transport.clone().out_link_events()))
        .abort_handle();

        Self {
            transport,
            peers,
            announce_task,
            recv_announce_task,
            recv_in_link_event_task,
        }
    }

    /// Send a chat message
    pub async fn chat(&self, message: String) {
        // Serialize message to msgpack
        let mut buf = Vec::new();
        message.serialize(&mut Serializer::new(&mut buf)).unwrap();
        let message_bytes = buf.as_slice();

        // Send message to all my peers
        for peer in self.peers.iter() {
            self.transport.send_to_out_links(peer.0, message_bytes).await;
        }
    }
}

/// Task that loops forever, sending my own announce packet every `config.anounce_interval_ms`
async fn announce_task(transport: Transport, in_destination: Arc<Mutex<SingleInputDestination>>, announce_info: AnnounceInfo) {
    let _ = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(5000));

        // Serialize AnnounceInfo to msgpack
        let mut buf = Vec::new();
        announce_info.clone().serialize(&mut Serializer::new(&mut buf)).unwrap();
        let announce_info_bytes_slice = buf.as_slice();

        loop {
            interval.tick().await;

            let packet = in_destination.clone().lock().await.announce(OsRng, Some(announce_info_bytes_slice)).unwrap();            
            let _ = transport.send_broadcast(packet).await;
            println!("\n***\nAnnounced myself as '{}' {}\n***\n", announce_info.nick, in_destination.clone().lock().await.identity.address_hash());

        }
    }).await;
}

/// Task that awaits receiving announce packets from others, and adds them to my FriendStore
async fn recv_announce_task(
    transport: Transport,
    mut peers: HashMap<AddressHash, Arc<Mutex<Link>>>,
    destination_name: DestinationName,
    in_destination: Arc<Mutex<SingleInputDestination>>,
    mut recv: Receiver<Arc<Mutex<SingleOutputDestination>>>
) {
    while let Ok(destination) = recv.recv().await {

        // Check if announce is for the same destination name
        let destination = destination.lock().await;
        if destination.desc.name.as_name_hash_slice() == destination_name.as_name_hash_slice() {
            println!("Received announce for my destination name");
            {
                let in_destination_lock = in_destination.lock().await;

                // Check if announce is already in the peer store, or is for my own identity
                if !peers.contains_key(&destination.identity.address_hash) && &destination.identity.address_hash != in_destination_lock.identity.address_hash() {
                    // Create a new Link with this peer
                    let link = transport.link(destination.desc).await;
                    {
                        let link_lock = link.lock().await;
                        println!("Created link to {}", link_lock.destination().address_hash)
                    }

                    // Add peer & Link to peers store
                    peers.insert(destination.identity.address_hash, link);

                    println!("Updated peers store: {:?}\n\n", peers.iter().map(|p| format!("{}", p.0)).collect::<Vec<String>>());
                } else {
                    println!("Already in peers store, ignoring...");
                }
            }
        } else {
            println!("Different destination name, ignoring...");
        }
    }
}

async fn recv_in_link_event_task(mut peers: HashMap<AddressHash, Arc<Mutex<Link>>>, mut recv: Receiver<LinkEventData>) {
    while let Ok(link_event_data) = recv.recv().await {
        match link_event_data.event {
            link::LinkEvent::Activated => {
                print!("Received Link Activated");
            },
            link::LinkEvent::Closed => {
                print!("Received Link Closed");
                peers.remove(&link_event_data.address_hash);
            },
            link::LinkEvent::Data(payload) => {
                let mut deserializer = Deserializer::new(Cursor::new(payload.as_slice()));
                if let Ok(Message(message)) = Deserialize::deserialize(&mut deserializer) {
                    print!("<< {}", message);
                } else {
                    print!("Received link data could not be deserialized, ignoring...");
                }
            }
        }
    }
}


async fn recv_out_link_event_task(mut peers: HashMap<AddressHash, Arc<Mutex<Link>>>, mut recv: Receiver<LinkEventData>) {
    while let Ok(link_event_data) = recv.recv().await {
        println!("OUT link event data")
    }
}