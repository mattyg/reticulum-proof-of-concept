use std::{collections::{BTreeSet, HashMap, HashSet}, sync::Arc, time::Duration};

use reticulum::{destination::{Destination, DestinationName, SingleInputDestination, SingleOutputDestination}, hash::AddressHash, identity::{Identity, PrivateIdentity}, iface::{tcp_client::TcpClient, tcp_server::TcpServer}, transport::Transport};
use rmp_serde::Serializer;
use serde::{Serialize, Deserialize};
use tokio::{task::AbortHandle, time, sync::{Mutex, broadcast::Receiver}};
use rand_core::OsRng;

/// A reticulum node that can chat
pub struct Chatter {
    transport: Transport,

    /// Others who are chatting about the same topic
    peers: Arc<Mutex<Vec<Identity>>>,
    
    announce_task: AbortHandle,
    recv_announce_task: AbortHandle,
}

impl Drop for Chatter {
    fn drop(&mut self) {
        self.announce_task.abort();
        self.recv_announce_task.abort();
    }
}

/// Info about this chatter to include in announce packet
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatterAnnounceInfo {
    nick: String
}

impl Chatter {
    pub async fn new(nick: String, topic: String) -> Self {
        // Create Identity
        let identity = PrivateIdentity::new_from_rand(OsRng);
        
        // Create empty peers list
        let peers = Arc::new(Mutex::new(Vec::new()));

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
        let announce_info = ChatterAnnounceInfo { nick };
        let announce_task = tokio::task::spawn(announce_task(transport.clone(), in_destination.clone(), announce_info))
            .abort_handle();

        // Handle incoming announce packets
        let recv_announce_task = tokio::task::spawn(recv_announce_task(peers.clone(), destination_name, in_destination.clone(), transport.clone().recv_announces().await))
            .abort_handle();

        Self {
            transport,
            peers,
            announce_task,
            recv_announce_task
        }
    }

    /// Send a chat message
    pub fn chat(&self, to: AddressHash, message: String) {
        
    }
}

/// Task that loops forever, sending my own announce packet every `config.anounce_interval_ms`
async fn announce_task(transport: Transport, in_destination: Arc<Mutex<SingleInputDestination>>, announce_info: ChatterAnnounceInfo) {
    let _ = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(5000));

        let mut announce_info_bytes = Vec::new();
        announce_info.clone().serialize(&mut Serializer::new(&mut announce_info_bytes)).unwrap();
        let announce_info_bytes_slice = announce_info_bytes.as_slice();

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
    peers: Arc<Mutex<Vec<Identity>>>,
    destination_name: DestinationName,
    in_destination: Arc<Mutex<SingleInputDestination>>,
    mut recv: Receiver<Arc<Mutex<SingleOutputDestination>>>
) {
    while let Ok(destination) = recv.recv().await {
        let destination = destination.lock().await;
        if destination.desc.name.as_name_hash_slice() == destination_name.as_name_hash_slice() {
            println!("Received announce for my destination name");
            {
                let mut peers_lock = peers.lock().await;
                let in_destination_lock = in_destination.lock().await;

                if !peers_lock.contains(&destination.identity) && &destination.identity.address_hash != in_destination_lock.identity.address_hash() {
                    peers_lock.push(destination.identity);
                    println!("Updated peers store: {:?}\n\n", peers_lock.iter().map(|p| format!("{}", p.address_hash)).collect::<Vec<String>>());
                } else {
                    println!("Already in peers store, ignoring...");
                }
            }
        } else {
            println!("Ignoring announce for different destination name {} from {}", destination_name.hash, destination.identity.address_hash);
        }
    }
}