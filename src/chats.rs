use std::str::FromStr;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use rand::{ rngs::SmallRng, SeedableRng, Rng };

use serde_json::Value;
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};
use enum_dispatch::enum_dispatch;
use colored::Colorize;
use chrono::NaiveDateTime;
use rustyline::ExternalPrinter;

use nostr::prelude::*;
use nostr::prelude::secp256k1::PublicKey;

use futures_util::{StreamExt, SinkExt};
use futures::stream::SplitStream;
use futures::stream::SplitSink;
use async_trait::async_trait;

use crate::crypto::{ RatchetProfile };

#[derive(Clone)]
#[enum_dispatch(Chat)] 
pub enum ChatType {
    PublicChannel(PublicChannel),
    PrivateChat(PrivateChat),
}

#[async_trait]
#[enum_dispatch] 
pub trait Chat {
    async fn print_incoming_events<T: ExternalPrinter + std::marker::Send + std::marker::Sync>(mut self, printing_helper: PrintingHandler<T>, reader: SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>);

    fn build_request_message(&self) -> Message;

    fn get_name(self) -> String;

    fn get_info_table(&self, relay: &str) -> String;

    fn message_from(&mut self, input: String, secret_key: SecretKey) -> Message;

    async fn get_next_message(&self, reader: &mut SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>) -> Result<Value, ()> {
        let message = match reader.next().await.unwrap() {
            Ok(val) => val,
            Err(why) => {
                panic!("Error while receiving message from Websocket. {}", why);
            }
        }.to_string();
        if message.is_empty() {
            return Err(());
        }
        let json_val: Value = match serde_json::from_str(&message) {
            Ok(val) => val,
            Err(why) => {
                eprintln!("Invalid JSON. {}", why);
                return Err(());
            }
        };
        Ok(json_val)
    }
}

#[derive(Clone)]
pub struct PublicChannel {
    pub root_event: Event,
    pub metadata: Metadata,
}

#[derive(Clone)]
pub struct PrivateChat {
    pub name: String,
    pub recipient_public_key: XOnlyPublicKey,
    pub secret_key: SecretKey,
    pub ratchet_profile: RatchetProfile, 
}

#[async_trait]
impl Chat for PublicChannel {
    async fn print_incoming_events<T: ExternalPrinter + std::marker::Send + std::marker::Sync>(mut self, mut printing_helper: PrintingHandler<T>, mut reader: SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>) {
            let mut history: Vec<Value> = Vec::new();

            // Print history first
            loop {
                let json_val = match self.get_next_message(&mut reader).await {
                    Ok(val) => val,
                    Err(_) => continue
                };

                let message_kind = json_val[0].as_str().unwrap();
                if message_kind == "EOSE" {
                   printing_helper.print_history(&mut history);
                   break;
                } 

                history.push(json_val);
            }

            // Print incoming messages second
            loop {
                let json_val = match self.get_next_message(&mut reader).await {
                    Ok(val) => val,
                    Err(_) => continue
                };
                printing_helper.print_message(json_val);
            }
    }

    fn build_request_message(&self) -> Message {
        let mut filter = Filter::default();
        filter.kinds = Some(vec![Kind::Custom(42)]);
        filter.events = Some(vec![self.root_event.id]);
        let req = ClientMessage::new_req(SubscriptionId::generate(), vec![filter]).as_json();
        return Message::Text(req)
    }

    fn get_name(self) -> String {
         return match self.metadata.name {
            Some(name) => {
                name
            },
            None => {
                self.root_event.id.to_hex().to_string()
            }
         }
    }

    fn message_from(&mut self, input: String, secret_key: SecretKey) -> Message {
//        let event: Event = EventBuilder::new_channel_msg(nostr::ChannelId::from_hex(self.root_event.id.to_hex()).unwrap(), url::Url::parse("").unwrap(), input).to_event(&Keys::new(secret_key)).unwrap();
        let event: Event = EventBuilder::new(Kind::Custom(42), input, &[Tag::Event(self.root_event.id, None, Some(Marker::Root))]).to_event(&Keys::new(secret_key)).unwrap();
        let client_msg = ClientMessage::new_event(event);
        Message::Text(client_msg.as_json())
    }

    fn get_info_table(&self, relay: &str) -> String {
        let relay = "Relay: ".green().to_string() + relay;
        let event_id_hex = "Event ID in Hex: ".green().to_string() + &self.root_event.id.to_hex();
        let event_id_bech32 = "Event ID in Bech32: ".green().to_string() + &self.root_event.id.to_bech32().unwrap();
        let channel_name = "Name: ".green().to_string() + match &self.metadata.name {
            Some(val) => &val,
            None => "No name"
        };
        let about = "About: ".green().to_string() + &self.metadata.about.as_ref().unwrap();
        let created_at = "Created at: ".green().to_string() + &NaiveDateTime::from_timestamp_opt(self.root_event.created_at.to_string().parse::<i64>().unwrap(), 0).unwrap().to_string();
        let creator = "Creator: ".green().to_string() + &self.root_event.pubkey.to_bech32().unwrap();
        return format!("{}\n{}\n{}\n{}\n{}\n{}\n{}", relay, channel_name, event_id_bech32, event_id_hex, about, creator, created_at)
    }
}

#[async_trait]
impl Chat for PrivateChat {
    async fn print_incoming_events<T: ExternalPrinter + std::marker::Send + std::marker::Sync>(mut self, mut printing_helper: PrintingHandler<T>, mut reader: SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>) {

            let mut _current_iteration: usize = 0;
            let mut history: Vec<Value> = Vec::new();

            // Print history first
            loop {
                let mut json_val = match self.get_next_message(&mut reader).await {
                    Ok(val) => val,
                    Err(_) => continue
                };
                let message_kind = json_val[0].as_str().unwrap();
                if message_kind == "EOSE" {
                   printing_helper.print_history(&mut history);
                   break;
                } 

                let pubkey = json_val[2]["pubkey"].to_string();
                println!("BEFORE CHANGING RECP KEY: {:?}", self.ratchet_profile.ephemeral_keys.lock().unwrap().recipient_public_key.serialize());
                self.ratchet_profile.ephemeral_keys.lock().unwrap().recipient_public_key = XOnlyPublicKey::from_str(&pubkey[ 1 .. pubkey.len() - 1]).unwrap().public_key(Parity::Even);
                println!("AFTER CHANGING RECP KEY: {:?}", self.ratchet_profile.ephemeral_keys.lock().unwrap().recipient_public_key.serialize());
                json_val[2]["content"] = serde_json::Value::String(self.ratchet_profile.decrypt_message(json_val[2]["content"].to_string()));
                history.push(json_val);
            }

            // Print incoming messages second
            loop {
                let mut json_val = match self.get_next_message(&mut reader).await {
                    Ok(val) => val,
                    Err(_) => continue
                };

                match json_val[0].as_str().unwrap() {
                    "EVENT" => {
                        let pubkey = json_val[2]["pubkey"].to_string();
                        self.ratchet_profile.ephemeral_keys.lock().unwrap().recipient_public_key = XOnlyPublicKey::from_str(&pubkey[ 1 .. pubkey.len() - 1]).unwrap().public_key(Parity::Even);
                        json_val[2]["content"] = serde_json::Value::String(self.ratchet_profile.decrypt_message(json_val[2]["content"].to_string()));
                        printing_helper.print_formatted_message(&json_val[2]["content"].to_string(), &json_val[2]["pubkey"].to_string());
                    }, 
                    "NOTICE" => {
                        eprintln!();
                    },
                    "OK" => {},
                    "EOSE" => {},
                    &_ => {
                        eprintln!("Unexpected event type: {}", json_val[0].as_str().unwrap()); 
                        continue;
                    }
                }
            }
    }

    fn build_request_message(&self) -> Message {
        let mut filter = Filter::default();
        filter.kinds = Some(vec![Kind::Custom(420)]);
       // filter.pubkeys = Some(vec![XOnlyPublicKey::from(self.recipient_public_key)]);
        let req = ClientMessage::new_req(SubscriptionId::generate(), vec![filter]).as_json();
        return Message::Text(req)
    }

    fn get_name(self) -> String {
        self.name
    }

    fn message_from(&mut self, input: String, secret_key: SecretKey) -> Message {
        let secp = Secp256k1::new();
        let mut rng = rand::thread_rng();
        let random_key = SecretKey::new(&mut rng);
        self.ratchet_profile.ephemeral_keys.lock().unwrap().secret_key = random_key;
        let enc_input = self.ratchet_profile.encrypt_message(input);
        let rec_pub_key = self.ratchet_profile.ephemeral_keys.lock().unwrap().recipient_public_key.x_only_public_key().0;
        let event: Event = EventBuilder::new(Kind::Custom(420), enc_input, &[Tag::PubKey(rec_pub_key, None)]).to_event(&Keys::new(random_key)).unwrap();
        let client_msg = ClientMessage::new_event(event);
        Message::Text(client_msg.as_json())
    }

    fn get_info_table(&self, relay: &str) -> String {
        format!("Coming soon!") //TODO: Implement this
    }
}

pub struct PrintingHandler<T> where T: ExternalPrinter {
    pub printer: T,
    pub pubkeys_to_colors: HashMap<String, u8>,
    pub public_key: XOnlyPublicKey,
}

impl<T: ExternalPrinter> PrintingHandler<T> {
    pub fn get_corresponding_color(&self, input: &str, number: u8) -> String {
        return match number {
           1 => input.green().to_string(),
           2 => input.red().to_string(),
           3 => input.blue().to_string(),
           4 => input.yellow().to_string(),
           5 => input.cyan().to_string(),
           6 => input.black().to_string(),
           7 => input.white().to_string(),
           8 => input.purple().to_string(),
           _ => {
                panic!("Couldn't generate color for message");
           }
        }
    }

    fn print_formatted_message(&mut self, message: &str, author_pubkey: &str) {
         if !self.pubkeys_to_colors.contains_key(author_pubkey) {
                let mut small_rng = SmallRng::from_entropy();
                self.pubkeys_to_colors.insert(author_pubkey.to_string(), small_rng.gen_range(1 .. 8));
            }
            let author_key_bech32 = XOnlyPublicKey::from_str(&author_pubkey[1 .. author_pubkey.len() - 1]).unwrap().to_bech32().unwrap();
            self.printer.print(format!("{}: {}", self.get_corresponding_color(&author_key_bech32[4 .. 10], self.pubkeys_to_colors[author_pubkey]), &message[1 .. message.len() - 1])).expect("Printing failed!");
    }

    pub fn print_history(&mut self, history: &mut Vec<Value>) {
         history.sort_by(|a, b| {
          let a_id = a[2]["created_at"].as_i64().unwrap();
          let b_id = b[2]["created_at"].as_i64().unwrap();
          a_id.cmp(&b_id)  
        });
          if history.len() != 0 {
               for i in 0 .. history.len()  {
                   let content = history[i][2]["content"].to_string();
                   self.print_formatted_message(&content, &history[i][2]["pubkey"].to_string());
               }
          }
    }

    pub fn print_message(&mut self, json_val: Value) {
           let message_kind = json_val[0].as_str().unwrap();
           match message_kind {
                 "EVENT" => {
                     let json_pubkey = json_val[2]["pubkey"].to_string();
                     if !(json_pubkey[1 .. json_pubkey.len() - 1] == self.public_key.to_string()) {
                        self.print_formatted_message(&json_val[2]["content"].to_string(), &json_val[2]["pubkey"].to_string());
                     }
                 },
                 "NOTICE" => {
                     println!("[{}] {}", "NOTICE".red(), &json_val[2]["content"]);
                 },
                 &_ => {

                 } 
           }
    }
}
